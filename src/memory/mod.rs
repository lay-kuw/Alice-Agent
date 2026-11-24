//! Cross-thread memory with provenance.
//!
//! Invariants (docs/SPEC.md §4.2):
//! 1. Every memory has >= 1 source; sourceless memories are deleted in the
//!    same transaction that removed their last source.
//! 2. Sources always reference an existing *user* message (FK).
//! 3. `memories.content` is derived: the newest remaining source's content.
//! 4. Extraction only reads user-authored text, never assistant output,
//!    tool output, or other memories' content.

use crate::providers::{self, ChatMessage, ChatRequest};
use crate::util::now_ms;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct Memory {
    pub id: i64,
    pub key: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub source_count: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub thread_id: String,
    pub thread_title: String,
    pub message_id: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Extracted {
    pub key: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub forget: bool,
}

/// Build an FTS5 query for the trigram tokenizer from free text.
/// ASCII words (>= 3 chars) are used whole; other scripts (Japanese etc.)
/// are split into overlapping 3-character grams.
pub fn fts_query(text: &str) -> Option<String> {
    fn add(terms: &mut Vec<String>, t: String) {
        if !terms.contains(&t) && terms.len() < 48 {
            terms.push(t);
        }
    }
    fn flush_ascii(w: &mut String, terms: &mut Vec<String>) {
        if w.len() >= 3 {
            add(terms, w.to_lowercase());
        }
        w.clear();
    }
    fn flush_other(o: &mut Vec<char>, terms: &mut Vec<String>) {
        for w in o.windows(3) {
            add(terms, w.iter().collect());
        }
        o.clear();
    }
    let mut terms: Vec<String> = Vec::new();
    let mut ascii = String::new();
    let mut other: Vec<char> = Vec::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            flush_other(&mut other, &mut terms);
            ascii.push(ch);
        } else if ch.is_alphanumeric() {
            flush_ascii(&mut ascii, &mut terms);
            other.push(ch);
        } else {
            flush_ascii(&mut ascii, &mut terms);
            flush_other(&mut other, &mut terms);
        }
    }
    flush_ascii(&mut ascii, &mut terms);
    flush_other(&mut other, &mut terms);
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

const MEM_COLS: &str = "m.id, m.key, m.content, m.created_at, m.updated_at, (SELECT COUNT(*) FROM memory_sources s WHERE s.memory_id = m.id)";

fn row_memory(r: &rusqlite::Row) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        key: r.get(1)?,
        content: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        source_count: r.get(5)?,
    })
}

pub fn search(c: &Connection, text: &str, limit: usize) -> rusqlite::Result<Vec<Memory>> {
    let Some(q) = fts_query(text) else { return Ok(vec![]) };
    let mut st = c.prepare_cached(&format!(
        "SELECT {MEM_COLS} FROM memories_fts f JOIN memories m ON m.id = f.rowid
         WHERE memories_fts MATCH ?1 ORDER BY bm25(memories_fts) LIMIT ?2"
    ))?;
    st.query_map(params![q, limit as i64], row_memory)?.collect()
}

pub fn list(c: &Connection, limit: usize, offset: usize) -> rusqlite::Result<Vec<Memory>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {MEM_COLS} FROM memories m ORDER BY m.updated_at DESC LIMIT ?1 OFFSET ?2"
    ))?;
    st.query_map(params![limit as i64, offset as i64], row_memory)?.collect()
}

pub fn get(c: &Connection, id: i64) -> rusqlite::Result<Option<(Memory, Vec<Source>)>> {
    let m = c
        .query_row(&format!("SELECT {MEM_COLS} FROM memories m WHERE m.id = ?1"), [id], row_memory)
        .optional()?;
    let Some(m) = m else { return Ok(None) };
    let mut st = c.prepare_cached(
        "SELECT s.thread_id, t.title, s.message_id, s.content, s.created_at
         FROM memory_sources s JOIN threads t ON t.id = s.thread_id
         WHERE s.memory_id = ?1 ORDER BY s.created_at DESC",
    )?;
    let sources = st
        .query_map([id], |r| {
            Ok(Source {
                thread_id: r.get(0)?,
                thread_title: r.get(1)?,
                message_id: r.get(2)?,
                content: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some((m, sources)))
}

/// Keys related to `text`, for key reuse during extraction (keys only).
pub fn related_keys(c: &Connection, text: &str, limit: usize) -> rusqlite::Result<Vec<String>> {
    Ok(search(c, text, limit)?.into_iter().map(|m| m.key).collect())
}

pub fn record_uses(c: &Connection, message_id: &str, memory_ids: &[i64]) -> rusqlite::Result<()> {
    let mut st = c.prepare_cached("INSERT OR IGNORE INTO memory_uses (message_id, memory_id) VALUES (?1, ?2)")?;
    for id in memory_ids {
        // Fails silently if either side was deleted meanwhile.
        let _ = st.execute(params![message_id, id]);
    }
    Ok(())
}

/// Recompute derived content from remaining sources; delete sourceless memories.
fn rebuild(c: &Connection, ids: &[i64]) -> rusqlite::Result<()> {
    let mut del = c.prepare_cached(
        "DELETE FROM memories WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM memory_sources WHERE memory_id = ?1)",
    )?;
    let mut upd = c.prepare_cached(
        "UPDATE memories SET
            content = (SELECT content FROM memory_sources WHERE memory_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1),
            updated_at = (SELECT MAX(created_at) FROM memory_sources WHERE memory_id = ?1)
         WHERE id = ?1 AND content IS NOT (SELECT content FROM memory_sources WHERE memory_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1)",
    )?;
    for id in ids {
        if del.execute([id])? == 0 {
            upd.execute([id])?;
        }
    }
    Ok(())
}

fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 64 && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Store extraction results for a user message. Atomic: if the message (or
/// its thread) has been deleted meanwhile, nothing is written.
/// Returns the IDs of memories created or updated.
pub fn apply_extraction(c: &mut Connection, message_id: &str, items: &[Extracted]) -> rusqlite::Result<Vec<i64>> {
    let tx = c.transaction()?;
    let msg: Option<(String, String)> = tx
        .query_row("SELECT thread_id, role FROM messages WHERE id = ?1", [message_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((thread_id, role)) = msg else { return Ok(vec![]) };
    if role != "user" {
        return Ok(vec![]);
    }
    let now = now_ms();
    let mut touched = Vec::new();
    for it in items {
        let key = it.key.trim();
        if !valid_key(key) {
            continue;
        }
        if it.forget {
            // The user asked to forget this topic: drop it with all sources.
            if let Some(id) = tx.query_row("SELECT id FROM memories WHERE key = ?1", [key], |r| r.get::<_, i64>(0)).optional()? {
                tx.execute("DELETE FROM memories WHERE id = ?1", [id])?;
            }
            continue;
        }
        let content = crate::util::truncate(it.content.trim(), 600);
        if content.is_empty() {
            continue;
        }
        tx.execute(
            "INSERT INTO memories (key, content, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(key) DO NOTHING",
            params![key, content, now],
        )?;
        let id: i64 = tx.query_row("SELECT id FROM memories WHERE key = ?1", [key], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO memory_sources (memory_id, message_id, thread_id, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(memory_id, message_id) DO UPDATE SET content = excluded.content",
            params![id, message_id, thread_id, content, now],
        )?;
        touched.push(id);
    }
    rebuild(&tx, &touched)?;
    tx.commit()?;
    Ok(touched)
}

/// Delete a thread and everything derived from it. Returns false if it did not exist.
/// Caller should afterwards run `storage::optimize_fts` + `storage::checkpoint`.
pub fn delete_thread(c: &mut Connection, thread_id: &str) -> rusqlite::Result<bool> {
    let tx = c.transaction()?;
    let affected: Vec<i64> = {
        let mut st = tx.prepare("SELECT DISTINCT memory_id FROM memory_sources WHERE thread_id = ?1")?;
        st.query_map([thread_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let n = tx.execute("DELETE FROM threads WHERE id = ?1", [thread_id])?;
    rebuild(&tx, &affected)?;
    tx.commit()?;
    Ok(n == 1)
}

/// Delete every thread and all conversation-derived data. Settings and
/// credentials live outside the database and are not touched.
pub fn delete_all(c: &mut Connection) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    tx.execute_batch(
        "DELETE FROM threads;
         DELETE FROM memories;
         DELETE FROM channel_messages;
         UPDATE channel_state SET thread_id = NULL;
         INSERT INTO memories_fts(memories_fts) VALUES ('rebuild');",
    )?;
    tx.commit()?;
    crate::storage::optimize_fts(c)?;
    crate::storage::vacuum(c)
}

/// Remove all memories (conversations stay). Not re-extracted from old messages.
pub fn clear(c: &mut Connection) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    tx.execute_batch("DELETE FROM memories; INSERT INTO memories_fts(memories_fts) VALUES ('rebuild');")?;
    tx.commit()?;
    crate::storage::optimize_fts(c)?;
    crate::storage::checkpoint(c)
}

#[derive(Serialize)]
pub struct Status {
    pub memories: i64,
    pub sources: i64,
    pub threads: i64,
    pub db_bytes: u64,
    pub attachments_bytes: u64,
}

pub fn status(c: &Connection, paths: &crate::config::Paths) -> rusqlite::Result<Status> {
    let q = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0));
    Ok(Status {
        memories: q("SELECT COUNT(*) FROM memories")?,
        sources: q("SELECT COUNT(*) FROM memory_sources")?,
        threads: q("SELECT COUNT(*) FROM threads")?,
        db_bytes: crate::storage::db_size_bytes(&paths.db()),
        attachments_bytes: dir_size(&paths.attachments()),
    })
}

fn dir_size(p: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Render memories for the system prompt. Clearly marked as data.
pub fn render_block(mems: &[Memory]) -> String {
    if mems.is_empty() {
        return String::new();
    }
    let mut s = String::from("<memories>\n");
    for m in mems {
        s.push_str(&format!("- {}\n", m.content.replace('\n', " ")));
    }
    s.push_str("</memories>");
    s
}

const EXTRACT_PROMPT: &str = r#"You maintain long-term memory about one user. Read the USER MESSAGE and extract durable facts about the user that will be useful in future, unrelated conversations: identity, preferences, relationships, work and projects, plans, important dates, constraints.

Rules:
- Only use facts the user states in the USER MESSAGE. EARLIER USER MESSAGES are context for resolving references only.
- Skip questions, one-off task requests, hypotheticals, small talk, and anything about the assistant.
- key: short snake_case topic id (a-z, 0-9, _). If the fact updates a topic in KNOWN KEYS, reuse that key.
- content: one concise, self-contained sentence in the user's language.
- If the user explicitly asks to forget something, output {"key": "<topic>", "forget": true}.
- Output JSON only: {"memories":[{"key":"...","content":"..."}]}. If nothing qualifies: {"memories":[]}"#;

pub fn extraction_request<'a>(model: &'a str, user_text: &str, earlier: &[String], known_keys: &[String]) -> ChatRequest<'a> {
    let mut input = String::new();
    if !known_keys.is_empty() {
        input.push_str(&format!("KNOWN KEYS: {}\n\n", known_keys.join(", ")));
    }
    if !earlier.is_empty() {
        input.push_str("EARLIER USER MESSAGES:\n");
        for e in earlier {
            input.push_str(&format!("- {}\n", crate::util::truncate(e, 500).replace('\n', " ")));
        }
        input.push('\n');
    }
    input.push_str("USER MESSAGE:\n");
    input.push_str(crate::util::truncate(user_text, 4000));
    ChatRequest {
        model,
        system: EXTRACT_PROMPT.to_string(),
        messages: vec![ChatMessage::user(input)],
        tools: vec![],
        max_tokens: 2000,
    }
}

pub fn parse_extraction(text: &str) -> Vec<Extracted> {
    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else { return vec![] };
    if b < a {
        return vec![];
    }
    let Ok(v) = serde_json::from_str::<Value>(&text[a..=b]) else { return vec![] };
    let Some(arr) = v.get("memories").and_then(|m| m.as_array()) else { return vec![] };
    arr.iter()
        .filter_map(|x| serde_json::from_value::<Extracted>(x.clone()).ok())
        .take(8)
        .collect()
}

/// Run one extraction call. Errors are returned to the caller for logging.
pub async fn extract(
    http: &reqwest::Client,
    provider: &crate::config::ProviderConfig,
    api_key: Option<&str>,
    model: &str,
    user_text: &str,
    earlier: &[String],
    known_keys: &[String],
) -> Result<Vec<Extracted>, providers::ProviderError> {
    let req = extraction_request(model, user_text, earlier, known_keys);
    let mut sink = |_: &str| {};
    let out = providers::stream(http, provider, api_key, &req, &mut sink).await?;
    Ok(parse_extraction(&out.text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_building() {
        assert_eq!(fts_query("hi"), None);
        assert_eq!(fts_query("Where do I live?").unwrap(), "\"where\" OR \"live\"");
        let q = fts_query("住所は").unwrap();
        assert_eq!(q, "\"住所は\"");
        let q = fts_query("私の住所").unwrap();
        assert!(q.contains("\"私の住\"") && q.contains("\"の住所\""));
        assert!(fts_query("a\"bc").is_none() || !fts_query("a\"bcd").unwrap().contains("\"\"\""));
    }

    #[test]
    fn parse() {
        let v = parse_extraction("```json\n{\"memories\":[{\"key\":\"home_city\",\"content\":\"大阪に住んでいる\"}]}\n```");
        assert_eq!(v, vec![Extracted { key: "home_city".into(), content: "大阪に住んでいる".into(), forget: false }]);
        assert!(parse_extraction("nothing").is_empty());
    }
}
