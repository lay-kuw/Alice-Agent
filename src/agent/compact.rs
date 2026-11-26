//! Rolling compaction: when a thread no longer fits the model's history
//! budget, the oldest part is folded into a short running summary (kept on
//! the thread) instead of being dropped. The model then sees the summary plus
//! the recent messages. A fold leaves room to spare, so it happens only now
//! and then, not on every turn.

use crate::app::AppRef;
use crate::providers::{self, ChatMessage, ChatRequest};
use crate::storage::Message;
use rusqlite::params;
use std::time::Duration;

/// After a fold, the kept history is about this share of the budget.
const KEEP_SHARE: f64 = 0.6;
/// Most text sent to the summarizer at once (the newest part of what is folded).
const INPUT_CAP: usize = 40_000;
const TIMEOUT: Duration = Duration::from_secs(90);

const PROMPT: &str = "You keep a running summary of a long conversation between a user and their assistant, Alice. \
Merge the previous summary (if any) with the new messages into one updated summary. Keep what later replies may need: \
the user's goals, decisions, preferences, facts, names, numbers, dates and open tasks. Drop small talk and tool details. \
Write at most 8 short lines in the language of the conversation. Output only the summary.";

/// Size of a message in the history budget (the same measure `build_history` uses).
fn size(m: &Message) -> usize {
    m.content.len() + 64
}

/// The messages to fold so the rest fits again: an index into `live` (exclusive)
/// that lands on a user message at or before the current turn. None when it all fits.
pub fn plan(live: &[Message], budget: usize, turn_start_seq: i64) -> Option<usize> {
    let total: usize = live.iter().map(size).sum();
    if total <= budget {
        return None;
    }
    let keep_target = (budget as f64 * KEEP_SHARE) as usize;
    // Walk back from the newest until the kept part reaches the target.
    let mut kept = 0usize;
    let mut cut = live.len();
    while cut > 0 && kept + size(&live[cut - 1]) <= keep_target {
        cut -= 1;
        kept += size(&live[cut]);
    }
    // Start the kept part at a user message, so tool results keep their call;
    // never fold the current turn.
    while cut < live.len() && live[cut].role != "user" {
        cut += 1;
    }
    let turn = live.iter().position(|m| m.seq >= turn_start_seq).unwrap_or(live.len());
    let cut = cut.min(turn);
    (cut > 0).then_some(cut)
}

fn transcript(previous: Option<&str>, fold: &[Message]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for m in fold {
        let who = match m.role.as_str() {
            "user" if m.client == "agent" => continue,
            "user" => "User",
            "assistant" => "Alice",
            "tool" => "Tool result",
            _ => continue,
        };
        let cap = if m.role == "tool" { 300 } else { 1500 };
        let text = crate::util::truncate(m.content.trim(), cap);
        if !text.is_empty() {
            lines.push(format!("[{who}] {text}"));
        }
    }
    // Keep the newest part when there is too much.
    let mut body = String::new();
    for l in lines.iter().rev() {
        if body.len() + l.len() > INPUT_CAP {
            break;
        }
        body.insert_str(0, &format!("{l}\n"));
    }
    match previous {
        Some(p) if !p.trim().is_empty() => format!("PREVIOUS SUMMARY:\n{p}\n\nNEW MESSAGES:\n{body}"),
        _ => format!("MESSAGES:\n{body}"),
    }
}

/// Fold the overflow of `msgs` into the thread's summary when needed.
/// Returns the summary to show the model and the messages still to send.
pub async fn prepare(app: &AppRef, thread_id: &str, msgs: Vec<Message>, budget: usize, turn_start_seq: i64, model_id: &str) -> (Option<String>, Vec<Message>) {
    let (summary, summary_seq): (Option<String>, i64) = app
        .db
        .lock()
        .query_row("SELECT summary, summary_seq FROM threads WHERE id = ?1", [thread_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap_or((None, 0));
    let live: Vec<Message> = msgs.into_iter().filter(|m| m.seq > summary_seq).collect();
    let Some(cut) = plan(&live, budget, turn_start_seq) else { return (summary, live) };

    let cfg = app.config();
    let model = if cfg.memory.model.is_empty() { model_id } else { cfg.memory.model.as_str() };
    let Some((provider, model_name)) = cfg.resolve_model(model) else { return (summary, live) };
    let api_key = app.secrets().provider_key(&provider.id).map(str::to_string);
    let req = ChatRequest {
        model: model_name,
        system: PROMPT.to_string(),
        messages: vec![ChatMessage::user(transcript(summary.as_deref(), &live[..cut]))],
        tools: vec![],
        max_tokens: 1200,
        mcp: None,
        effort: None,
        tool_runner: None,
    };
    let mut sink = |_: &str| {};
    let out = tokio::time::timeout(TIMEOUT, providers::stream(&app.http, provider, api_key.as_deref(), &req, &mut sink)).await;
    let text = match out {
        Ok(Ok(o)) if !o.text.trim().is_empty() => o.text.trim().to_string(),
        Ok(Err(e)) => {
            crate::warn!("compaction failed thread={thread_id}: {e}");
            return (summary, live);
        }
        _ => {
            crate::warn!("compaction gave nothing thread={thread_id}");
            return (summary, live);
        }
    };
    let new_seq = live[cut - 1].seq;
    let _ = app.db.lock().execute(
        "UPDATE threads SET summary = ?2, summary_seq = ?3 WHERE id = ?1",
        params![thread_id, crate::util::truncate(&text, 4000), new_seq],
    );
    crate::info!("compacted thread={thread_id} up to seq {new_seq} ({cut} messages)");
    (Some(text), live[cut..].to_vec())
}

/// The summary as it goes into the system prompt.
pub fn render(summary: &str) -> String {
    format!(
        "<earlier_context_summary>\n{}\n</earlier_context_summary>\n(Older messages of this conversation are summarized above; the recent ones follow.)\n",
        summary.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(seq: i64, role: &str, len: usize) -> Message {
        Message {
            id: format!("m{seq}"),
            thread_id: "t".into(),
            seq,
            role: role.into(),
            content: "x".repeat(len),
            tool_calls: None,
            tool_call_id: None,
            status: "complete".into(),
            client: "web".into(),
            model: None,
            provider_state: None,
            meta: None,
            created_at: 0,
        }
    }

    #[test]
    fn folds_only_when_over_budget_and_at_a_user_message() {
        let small: Vec<Message> = (1..=4).map(|i| msg(i, if i % 2 == 1 { "user" } else { "assistant" }, 100)).collect();
        assert_eq!(plan(&small, 10_000, 3), None);
        // 20 messages of ~1064 = ~21k; budget 10k keeps ~6k.
        let roles = ["user", "assistant", "tool", "assistant"];
        let long: Vec<Message> = (1..=20).map(|i| msg(i, roles[(i as usize - 1) % 4], 1000)).collect();
        let cut = plan(&long, 10_000, 20).unwrap();
        assert_eq!(long[cut].role, "user");
        let kept: usize = long[cut..].iter().map(size).sum();
        assert!(kept <= 6_400, "{kept}");
        // Never folds into the current turn.
        let cut = plan(&long, 10_000, 5).unwrap();
        assert!(long[cut].seq <= 5);
    }

    #[test]
    fn transcript_skips_agent_instructions_and_keeps_the_newest() {
        let mut m = vec![msg(1, "user", 10), msg(2, "assistant", 10)];
        m[0].client = "agent".into();
        let t = transcript(Some("old facts"), &m);
        assert!(t.starts_with("PREVIOUS SUMMARY:\nold facts") && !t.contains("[User]") && t.contains("[Alice]"));
    }
}
