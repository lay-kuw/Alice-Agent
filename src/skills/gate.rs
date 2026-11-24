//! The Tool Gate: the only way any model reaches a tool.
//!
//! Native tool calling, the MCP relay and the text protocol all call
//! [`invoke`]. It resolves the tool in the registry, applies the user's
//! permission, waits for approval in Alice's own UI, runs the handler,
//! scrubs credentials from the output and records the run. There is no
//! parameter that skips approval.

use super::{GUIDE_TOOL, auth, handlers, registry};
use crate::app::{AppRef, PendingApproval};
use crate::config::Permission;
use crate::util::{new_id, now_ms};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Native,
    Mcp,
    Text,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Native => "native",
            Source::Mcp => "mcp",
            Source::Text => "text",
        }
    }
}

pub struct Call<'a> {
    pub thread_id: &'a str,
    pub message_id: &'a str,
    pub call_id: &'a str,
    pub name: &'a str,
    pub args: &'a Value,
    pub source: Source,
    pub client: &'a str,
}

pub enum Outcome {
    /// (content for the model, ok)
    Done(String, bool),
    Cancelled,
}

pub async fn invoke(app: &AppRef, call: &Call<'_>, cancel: &CancellationToken) -> Outcome {
    let started = Instant::now();
    let reg = registry(app);

    if call.name == GUIDE_TOOL {
        let id = call.args.get("skill").and_then(|v| v.as_str()).unwrap_or("");
        return match reg.usable_skills().into_iter().find(|s| s.id() == id) {
            Some(s) if !s.guide.trim().is_empty() => Outcome::Done(s.guide.clone(), true),
            Some(s) => Outcome::Done(format!("{}: {}", s.manifest.name, s.manifest.description), true),
            None => Outcome::Done(format!("error: unknown or unavailable skill '{id}'"), false),
        };
    }

    let Some(tool) = reg.find(call.name).cloned() else {
        record(app, call, "", "error", started, Some("unknown tool"));
        return Outcome::Done(format!("error: unknown tool '{}'", call.name), false);
    };
    let skill = tool.skill.clone();
    if !tool.available {
        record(app, call, skill.id(), "unavailable", started, Some(&tool.reason));
        return Outcome::Done(format!("error: tool '{}' is not available: {}", call.name, tool.reason), false);
    }
    if tool.permission == Permission::Disabled {
        record(app, call, skill.id(), "denied", started, None);
        return Outcome::Done(format!("error: tool '{}' is disabled by the user", call.name), false);
    }
    if tool.permission == Permission::Ask {
        match approve(app, call, &skill.manifest.name, cancel).await {
            None => {
                record(app, call, skill.id(), "cancelled", started, None);
                return Outcome::Cancelled;
            }
            Some(false) => {
                record(app, call, skill.id(), "declined", started, None);
                return Outcome::Done("The user declined to run this tool.".into(), false);
            }
            Some(true) => {}
        }
    }

    app.emit(json!({
        "type": "tool.started", "thread_id": call.thread_id, "message_id": call.message_id,
        "tool_call_id": call.call_id, "tool": call.name, "arguments": call.args, "skill": skill.id(),
    }));
    let Some(manifest) = skill.tool(call.name).cloned() else {
        return Outcome::Done("error: tool vanished".into(), false);
    };
    let result = tokio::select! {
        r = handlers::run(app, &skill, &manifest, call.args) => r,
        _ = cancel.cancelled() => {
            record(app, call, skill.id(), "cancelled", started, None);
            return Outcome::Cancelled;
        }
    };
    let secrets = auth::secrets_of(app, &skill);
    let scrub = |s: String| secrets.iter().fold(s, |acc, t| acc.replace(t.as_str(), "[redacted]"));
    match result {
        Ok(out) => {
            let out = scrub(out);
            record(app, call, skill.id(), "ok", started, None);
            Outcome::Done(out, true)
        }
        Err(e) => {
            let e = scrub(e);
            record(app, call, skill.id(), "error", started, Some(&e));
            Outcome::Done(format!("error: {e}"), false)
        }
    }
}

/// Ask the user. `None` = the turn was cancelled while waiting.
async fn approve(app: &AppRef, call: &Call<'_>, skill_name: &str, cancel: &CancellationToken) -> Option<bool> {
    let approval_id = new_id();
    let (tx, rx) = oneshot::channel();
    app.approvals
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(approval_id.clone(), PendingApproval { thread_id: call.thread_id.to_string(), reply: tx });
    app.emit(json!({
        "type": "tool.approval_required", "thread_id": call.thread_id, "message_id": call.message_id,
        "approval_id": approval_id, "tool": call.name, "arguments": call.args, "skill": skill_name,
        "tool_call_id": call.call_id, "client": call.client, "via": call.source.as_str(),
    }));
    let decision = tokio::select! {
        r = tokio::time::timeout(APPROVAL_TIMEOUT, rx) => match r { Ok(Ok(b)) => Some(b), _ => Some(false) },
        _ = cancel.cancelled() => None,
    };
    app.approvals.lock().unwrap_or_else(|e| e.into_inner()).remove(&approval_id);
    app.emit(json!({ "type": "tool.approval_resolved", "thread_id": call.thread_id, "approval_id": approval_id, "approved": decision == Some(true) }));
    decision
}

fn record(app: &AppRef, call: &Call<'_>, skill: &str, status: &str, started: Instant, error: Option<&str>) {
    let ms = started.elapsed().as_millis() as i64;
    let error = error.map(|e| crate::util::truncate(e, 500).to_string());
    crate::info!("tool {} skill={} via={} status={} {}ms", call.name, skill, call.source.as_str(), status, ms);
    let c = app.db.lock();
    let _ = c.execute(
        "INSERT INTO tool_runs (thread_id, skill, tool, source, status, duration_ms, error, created_at)
         VALUES ((SELECT id FROM threads WHERE id = ?1), ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![call.thread_id, skill, call.name, call.source.as_str(), status, ms, error, now_ms()],
    );
}
