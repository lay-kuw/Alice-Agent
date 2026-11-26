//! Looking back: Alice's memories and earlier chats, searched on this
//! machine only (SQLite full-text search for memories, plain matching for
//! messages). The model picks the keywords, which covers vague questions
//! ("that cafe we talked about") without embeddings or outside services.

use crate::app::App;
use crate::memory;
use rusqlite::Connection;
use serde_json::{Value, json};

pub async fn execute(app: &App, name: &str, args: &Value) -> Result<String, String> {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if query.is_empty() {
        return Err("query is required".into());
    }
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(8).clamp(1, 20) as usize;
    let c = app.db.lock();
    let out = match name {
        "memory_search" => memories(&c, &query, limit),
        "conversation_search" => conversations(&c, &query, limit),
        _ => return Err(format!("unknown tool {name}")),
    }
    .map_err(|e| e.to_string())?;
    // Old chats can quote web pages and mail: data, not instructions.
    Ok(super::untrusted("history", &serde_json::to_string_pretty(&out).unwrap_or_default()))
}

/// Keywords of a query: words of two characters or more (a query without
/// spaces, as Japanese often is, is one keyword).
fn terms(query: &str) -> Vec<String> {
    let mut t: Vec<String> = query.split_whitespace().filter(|w| w.chars().count() >= 2).map(str::to_lowercase).collect();
    t.dedup();
    t.truncate(8);
    t
}

fn memories(c: &Connection, query: &str, limit: usize) -> rusqlite::Result<Value> {
    // Each keyword on its own too, so one unknown word does not hide the rest.
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for q in std::iter::once(query.to_string()).chain(terms(query)) {
        for m in memory::search(c, &q, limit)? {
            if seen.insert(m.id) && found.len() < limit {
                found.push(json!({ "key": m.key, "content": m.content, "updated": crate::util::fmt_utc(m.updated_at)[..10].to_string() }));
            }
        }
    }
    Ok(json!({ "memories": found }))
}

/// content, role, created_at, chat title, chat id
type Hit = (String, String, i64, String, String);

fn conversations(c: &Connection, query: &str, limit: usize) -> rusqlite::Result<Value> {
    let terms = terms(query);
    if terms.is_empty() {
        return Ok(json!({ "matches": [] }));
    }
    // Messages containing any keyword (instructions of scheduled agents and
    // tool output are not conversation), newest first; rank by keywords hit.
    let cond = terms.iter().enumerate().map(|(i, _)| format!("instr(lower(m.content), ?{}) > 0", i + 1)).collect::<Vec<_>>().join(" OR ");
    let sql = format!(
        "SELECT m.content, m.role, m.created_at, t.title, t.id FROM messages m JOIN threads t ON t.id = m.thread_id
         WHERE m.role IN ('user', 'assistant') AND m.client NOT IN ('agent', 'background') AND m.status = 'complete' AND ({cond})
         ORDER BY m.created_at DESC LIMIT 400"
    );
    let mut st = c.prepare(&sql)?;
    let rows: Vec<Hit> = st
        .query_map(rusqlite::params_from_iter(terms.iter()), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    let mut scored: Vec<(usize, Hit)> = rows
        .into_iter()
        .map(|r| {
            let low = r.0.to_lowercase();
            (terms.iter().filter(|t| low.contains(t.as_str())).count(), r)
        })
        .collect();
    // More keywords first; the order from SQL (newest first) breaks ties.
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    let matches: Vec<Value> = scored
        .into_iter()
        .take(limit)
        .map(|(_, (content, role, at, title, id))| {
            json!({
                "chat": if title.is_empty() { "(untitled)".to_string() } else { title },
                "chat_id": id,
                "date": crate::util::fmt_utc(at)[..16].to_string(),
                "from": if role == "user" { "user" } else { "Alice" },
                "text": snippet(&content, &terms),
            })
        })
        .collect();
    Ok(json!({ "matches": matches }))
}

/// About 300 characters around the first keyword found.
fn snippet(text: &str, terms: &[String]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let low: Vec<char> = text.to_lowercase().chars().collect();
    let pos = terms
        .iter()
        .filter_map(|t| {
            let tc: Vec<char> = t.chars().collect();
            low.windows(tc.len().max(1)).position(|w| w == tc.as_slice())
        })
        .min()
        .unwrap_or(0);
    let start = pos.saturating_sub(100);
    let end = (start + 300).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s.insert(0, '…');
    }
    if end < chars.len() {
        s.push('…');
    }
    s.replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{self, NewMessage};

    fn add(c: &Connection, thread: &str, role: &str, client: &str, text: &str) {
        storage::insert_message(
            c,
            &NewMessage { thread_id: thread, role, content: text, tool_calls: None, tool_call_id: None, status: "complete", client, model: None },
        )
        .unwrap();
    }

    #[test]
    fn finds_earlier_chats_by_keywords() {
        let db = storage::Db::open_in_memory().unwrap();
        let c = db.lock();
        let t = storage::create_thread(&c, "週末の予定", None).unwrap();
        add(&c, &t.id, "user", "web", "代官山のカフェ「ミモザ」がよかった！ケーキが最高");
        add(&c, &t.id, "assistant", "web", "ミモザ、素敵ですね。また行きましょう");
        add(&c, &t.id, "user", "agent", "カフェ情報を調べて"); // a scheduled instruction: not conversation
        let other = storage::create_thread(&c, "仕事", None).unwrap();
        add(&c, &other.id, "user", "web", "会議は金曜");

        let v = conversations(&c, "カフェ ケーキ", 5).unwrap();
        let m = v["matches"].as_array().unwrap();
        assert_eq!(m.len(), 1, "{v}");
        assert_eq!(m[0]["chat"], "週末の予定");
        assert!(m[0]["text"].as_str().unwrap().contains("ミモザ"));
        // Case-insensitive, and a single word works.
        assert_eq!(conversations(&c, "ミモザ", 5).unwrap()["matches"].as_array().unwrap().len(), 2);
        assert!(conversations(&c, "x", 5).unwrap()["matches"].as_array().unwrap().is_empty());
    }

    #[test]
    fn snippets_center_on_the_keyword() {
        let long = format!("{}カフェ{}", "あ".repeat(500), "い".repeat(500));
        let s = snippet(&long, &["カフェ".into()]);
        assert!(s.starts_with('…') && s.ends_with('…') && s.contains("カフェ") && s.chars().count() < 310);
    }
}
