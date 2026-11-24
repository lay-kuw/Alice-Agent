//! The agent loop: build context, stream from the provider, run tools,
//! persist, and extract memories. Every write depends on the thread still
//! existing (foreign keys), so a concurrent delete cannot be undone by a
//! generation that is still running.

use crate::app::{ActiveGeneration, AppRef, Bridge};
use crate::config::{Config, ProviderKind};
use crate::memory;
use crate::providers::{self, ChatMessage, ChatRequest, Image, Role, ToolCall};
use crate::storage::{self, Message, MessageUpdate, NewMessage};
use crate::skills::{self, gate};
use crate::tools;
use crate::util::now_ms;
use crate::{error, warn};
use serde_json::{Value, json};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub enum TurnError {
    NotFound,
    Busy,
    BadRequest(String),
    Internal(String),
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TurnError::NotFound => f.write_str("thread not found"),
            TurnError::Busy => f.write_str("a response is already being generated in this thread"),
            TurnError::BadRequest(m) | TurnError::Internal(m) => f.write_str(m),
        }
    }
}

pub struct TurnInput {
    pub thread_id: String,
    pub text: String,
    pub client: String,
    /// Per-message model override (also stored on the thread).
    pub model: Option<String>,
    pub attachment_ids: Vec<String>,
}

pub struct TurnStarted {
    pub user_message: Message,
    pub assistant_message: Message,
    pub model: String,
}

/// Resolve which model a thread uses.
pub fn effective_model(cfg: &Config, thread_model: Option<&str>, override_model: Option<&str>) -> Result<String, TurnError> {
    let m = override_model
        .filter(|m| !m.is_empty())
        .or(thread_model.filter(|m| !m.is_empty()))
        .unwrap_or(cfg.agent.default_model.as_str());
    if m.is_empty() {
        return Err(TurnError::BadRequest("no model configured (run `alice model add`)".into()));
    }
    if cfg.resolve_model(m).is_none() {
        return Err(TurnError::BadRequest(format!("unknown model: {m}")));
    }
    Ok(m.to_string())
}

pub fn start_turn(app: &AppRef, input: TurnInput) -> Result<TurnStarted, TurnError> {
    if input.text.trim().is_empty() && input.attachment_ids.is_empty() {
        return Err(TurnError::BadRequest("empty message".into()));
    }
    let cfg = app.config();
    let cancel = CancellationToken::new();
    // Reserve the thread before touching the DB so two clients cannot start
    // concurrent generations in the same thread.
    {
        let mut active = app.active.lock().unwrap_or_else(|e| e.into_inner());
        if active.contains_key(&input.thread_id) {
            return Err(TurnError::Busy);
        }
        active.insert(input.thread_id.clone(), ActiveGeneration { cancel: cancel.clone(), client: input.client.clone() });
    }
    let release = |app: &AppRef| {
        app.active.lock().unwrap_or_else(|e| e.into_inner()).remove(&input.thread_id);
    };

    let result = (|| {
        let mut c = app.db.lock();
        let thread = storage::get_thread(&c, &input.thread_id)
            .map_err(|e| TurnError::Internal(e.to_string()))?
            .ok_or(TurnError::NotFound)?;
        let model = effective_model(&cfg, thread.model.as_deref(), input.model.as_deref())?;
        let tx = c.transaction().map_err(|e| TurnError::Internal(e.to_string()))?;
        let user = storage::insert_message(
            &tx,
            &NewMessage {
                thread_id: &thread.id,
                role: "user",
                content: &input.text,
                tool_calls: None,
                tool_call_id: None,
                status: "complete",
                client: &input.client,
                model: None,
            },
        )
        .map_err(|e| TurnError::Internal(e.to_string()))?;
        for aid in &input.attachment_ids {
            tx.execute(
                "UPDATE attachments SET message_id = ?1 WHERE id = ?2 AND thread_id = ?3 AND message_id IS NULL",
                rusqlite::params![user.id, aid, thread.id],
            )
            .map_err(|e| TurnError::Internal(e.to_string()))?;
        }
        let assistant = storage::insert_message(
            &tx,
            &NewMessage {
                thread_id: &thread.id,
                role: "assistant",
                content: "",
                tool_calls: None,
                tool_call_id: None,
                status: "streaming",
                client: &input.client,
                model: Some(&model),
            },
        )
        .map_err(|e| TurnError::Internal(e.to_string()))?;
        let mut renamed = None;
        if thread.title.is_empty() {
            let title = auto_title(&input.text);
            tx.execute("UPDATE threads SET title = ?2 WHERE id = ?1", rusqlite::params![thread.id, title])
                .map_err(|e| TurnError::Internal(e.to_string()))?;
            renamed = Some(title);
        }
        if input.model.as_deref().is_some_and(|m| Some(m) != thread.model.as_deref()) {
            tx.execute("UPDATE threads SET model = ?2 WHERE id = ?1", rusqlite::params![thread.id, model])
                .map_err(|e| TurnError::Internal(e.to_string()))?;
        }
        storage::touch_thread(&tx, &thread.id).map_err(|e| TurnError::Internal(e.to_string()))?;
        tx.commit().map_err(|e| TurnError::Internal(e.to_string()))?;
        let thread = storage::get_thread(&c, &thread.id).ok().flatten();
        Ok((user, assistant, model, renamed.and(thread)))
    })();

    let (user, assistant, model, renamed) = match result {
        Ok(v) => v,
        Err(e) => {
            release(app);
            return Err(e);
        }
    };

    app.emit(json!({ "type": "message.created", "thread_id": user.thread_id, "message": user }));
    app.emit(json!({ "type": "message.created", "thread_id": assistant.thread_id, "message": assistant }));
    if let Some(t) = renamed {
        app.emit(json!({ "type": "thread.updated", "thread_id": t.id, "thread": t }));
    }

    let started = TurnStarted { user_message: user.clone(), assistant_message: assistant.clone(), model: model.clone() };
    let app2 = app.clone();
    tokio::spawn(async move {
        let thread_id = user.thread_id.clone();
        let outcome = run_turn(&app2, &user, assistant, &model, cancel).await;
        app2.active.lock().unwrap_or_else(|e| e.into_inner()).remove(&thread_id);
        app2.drop_approvals(Some(&thread_id));
        if outcome == Outcome::Completed {
            extract_memories(&app2, &user, &model).await;
        }
    });
    Ok(started)
}

fn auto_title(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("New chat").trim();
    let mut t: String = line.chars().take(40).collect();
    if line.chars().count() > 40 {
        t.push('…');
    }
    t
}

#[derive(PartialEq, Eq, Debug)]
enum Outcome {
    Completed,
    Cancelled,
    Failed,
    Deleted,
}

/// The system prompt is SOUL.md (with `{{name}}` filled in) plus the facts
/// only Alice knows: the current time and memories relevant to this message.
fn system_prompt(cfg: &Config, soul: &str, memories: &str, workspace: Option<&std::path::Path>, skills: &str) -> String {
    let mut s = soul.trim().replace("{{name}}", &cfg.agent.name);
    s.push_str(&format!("\n\nCurrent time (UTC): {}\n", &crate::util::fmt_utc(now_ms())[..16]));
    if let Some(w) = workspace {
        s.push_str(&format!("Working folder for file tools: {}\n", w.display()));
    }
    if !skills.is_empty() {
        s.push('\n');
        s.push_str(skills);
    }
    if !memories.is_empty() {
        s.push('\n');
        s.push_str(memories);
        s.push('\n');
    }
    s
}

/// Convert stored messages into provider messages within a character budget.
/// `turn_start_seq`: messages at or after this seq belong to the current turn
/// and keep their provider replay state.
pub fn build_history(msgs: &[Message], budget: usize, turn_start_seq: i64, images: &dyn Fn(&str) -> Vec<Image>) -> Vec<ChatMessage> {
    use std::collections::HashSet;
    let answered: HashSet<&str> = msgs
        .iter()
        .filter(|m| m.role == "tool")
        .filter_map(|m| m.tool_call_id.as_deref())
        .collect();
    let mut out: Vec<ChatMessage> = Vec::new();
    let mut used = 0usize;
    for m in msgs.iter().rev() {
        let mut cm = match m.role.as_str() {
            "user" => {
                let mut cm = ChatMessage::user(m.content.clone());
                cm.images = images(&m.id);
                cm
            }
            "assistant" => {
                if m.status == "streaming" || m.status == "error" {
                    continue;
                }
                let calls: Vec<ToolCall> = m
                    .tool_calls
                    .as_ref()
                    .and_then(|v| serde_json::from_value::<Vec<ToolCall>>(v.clone()).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|c| answered.contains(c.id.as_str()))
                    .collect();
                if m.content.is_empty() && calls.is_empty() {
                    continue;
                }
                let mut cm = ChatMessage::assistant(m.content.clone());
                cm.tool_calls = calls;
                cm
            }
            "tool" => {
                let mut cm = ChatMessage::user(m.content.clone());
                cm.role = Role::Tool;
                cm.tool_call_id = m.tool_call_id.clone();
                cm.tool_name = m.meta.as_ref().and_then(|v| v["tool"].as_str()).map(str::to_string);
                cm
            }
            _ => continue,
        };
        if m.seq >= turn_start_seq {
            cm.provider_state = m.provider_state.clone();
        }
        used += cm.content.len() + 64;
        out.push(cm);
        if used > budget && m.role == "user" {
            break;
        }
    }
    out.reverse();
    // Start at a user message so tool results never lose their call.
    while out.first().is_some_and(|m| m.role != Role::User) {
        out.remove(0);
    }
    out
}

fn load_images(app: &AppRef, message_id: &str) -> Vec<Image> {
    let rows: Vec<(String, String, String)> = {
        let c = app.db.lock();
        let Ok(mut st) = c.prepare_cached("SELECT id, thread_id, mime FROM attachments WHERE message_id = ?1 AND mime LIKE 'image/%'") else {
            return vec![];
        };
        st.query_map([message_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|it| it.flatten().collect())
            .unwrap_or_default()
    };
    rows.into_iter()
        .filter_map(|(id, thread_id, mime)| {
            let bytes = std::fs::read(app.paths.attachments().join(&thread_id).join(&id)).ok()?;
            Some(Image { mime, data_b64: crate::util::b64_encode(&bytes) })
        })
        .collect()
}

/// Text of non-image attachments, appended to the user message.
fn attachment_text(app: &AppRef, message_id: &str) -> String {
    let rows: Vec<(String, String, String, String)> = {
        let c = app.db.lock();
        let Ok(mut st) = c.prepare_cached(
            "SELECT id, thread_id, name, mime FROM attachments WHERE message_id = ?1 AND mime NOT LIKE 'image/%'",
        ) else {
            return String::new();
        };
        st.query_map([message_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map(|it| it.flatten().collect())
            .unwrap_or_default()
    };
    let mut s = String::new();
    for (id, thread_id, name, _mime) in rows {
        let Ok(bytes) = std::fs::read(app.paths.attachments().join(&thread_id).join(&id)) else { continue };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            s.push_str(&format!("\n[attached binary file: {name}, {} bytes — content not readable as text]", bytes.len()));
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        s.push_str(&format!("\n\n{}", tools::untrusted(&format!("attachment:{name}"), crate::util::truncate(&text, 200_000))));
    }
    s
}

async fn run_turn(app: &AppRef, user: &Message, first: Message, model_id: &str, cancel: CancellationToken) -> Outcome {
    let t_turn = Instant::now();
    let mut assistant = first;
    let mut provider_ms_total: u128 = 0;
    let mut step = 0usize;
    loop {
        let t_step = Instant::now();
        let cfg = app.config();
        let secrets = app.secrets();
        let Some((provider, model_name)) = cfg.resolve_model(model_id) else {
            fail(app, &assistant, "", &format!("unknown model: {model_id}"));
            return Outcome::Failed;
        };
        let provider = provider.clone();
        let api_key = secrets.provider_key(&provider.id).map(str::to_string);
        // Every provider gets the same registry: native tool calling, MCP
        // (Codex) or the text protocol (other CLIs) — all through the gate.
        let reg = skills::registry(app);
        let tool_specs = reg.specs();
        let ws = tool_specs.iter().any(|t| t.name.starts_with("fs_")).then(|| tools::fs::workspace_dir(&cfg, &app.paths));
        let skill_index = reg.index();

        // Context: memories relevant to this user message + thread history.
        let (history, mem_ids, mem_block) = {
            let c = app.db.lock();
            let mems = memory::search(&c, &user.content, cfg.memory.inject_limit).unwrap_or_default();
            let ids: Vec<i64> = mems.iter().map(|m| m.id).collect();
            let block = memory::render_block(&mems);
            let msgs = match storage::list_messages(&c, &user.thread_id) {
                Ok(m) => m,
                Err(_) => return Outcome::Deleted,
            };
            drop(c);
            let img = |id: &str| if id == user.id { load_images(app, id) } else { vec![] };
            let mut h = build_history(&msgs, cfg.agent.history_chars, user.seq, &img);
            let extra = attachment_text(app, &user.id);
            if !extra.is_empty()
                && let Some(last_user) = h.iter_mut().rev().find(|m| m.role == Role::User) {
                    last_user.content.push_str(&extra);
                }
            (h, ids, block)
        };
        let req = ChatRequest {
            model: model_name,
            system: system_prompt(&cfg, &crate::config::read_soul(&app.paths), &mem_block, ws.as_deref(), &skill_index),
            messages: history,
            tools: tool_specs,
            max_tokens: cfg.agent.max_tokens,
            mcp: None,
            effort: crate::config::split_effort(model_id).1.map(str::to_string),
        };
        let mut req = req;
        // Codex reaches tools through `alice mcp` with a token for this turn only.
        let bridge = (provider.kind == ProviderKind::CodexCli && !req.tools.is_empty()).then(|| {
            app.open_bridge(Bridge {
                thread_id: user.thread_id.clone(),
                message_id: assistant.id.clone(),
                client: user.client.clone(),
                cancel: cancel.child_token(),
                calls: Default::default(),
            })
        });
        if let Some((token, _)) = &bridge {
            req.mcp = Some(providers::McpBridge { token: token.clone(), url: crate::cli::client::local_url(&cfg), profile: app.paths.profile.clone() });
        }
        let alice_us = t_step.elapsed().as_micros();
        let alice_ms = alice_us / 1000;

        // Stream.
        let mut partial = String::new();
        let mut ttft: Option<u128> = None;
        let t_provider = Instant::now();
        let thread_id = user.thread_id.clone();
        let message_id = assistant.id.clone();
        let result = {
            let mut on_delta = |d: &str| {
                if ttft.is_none() {
                    ttft = Some(t_provider.elapsed().as_millis());
                }
                partial.push_str(d);
                app.emit(json!({ "type": "message.delta", "thread_id": thread_id, "message_id": message_id, "delta": d }));
            };
            tokio::select! {
                r = providers::stream(&app.http, &provider, api_key.as_deref(), &req, &mut on_delta) => Some(r),
                _ = cancel.cancelled() => None,
            }
        };
        let provider_ms = t_provider.elapsed().as_millis();
        provider_ms_total += provider_ms;
        // Calls Codex already made through the bridge (executed, not to be re-run).
        let bridged: Vec<ToolCall> = match &bridge {
            Some((token, b)) => {
                b.cancel.cancel();
                app.close_bridge(token);
                b.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
            }
            None => vec![],
        };

        let completion = match result {
            None => {
                finish_cancelled(app, &assistant, &partial);
                return Outcome::Cancelled;
            }
            Some(Err(e)) => {
                fail(app, &assistant, &partial, &e.0);
                return Outcome::Failed;
            }
            Some(Ok(c)) => c,
        };

        let t_post = Instant::now();
        let all_calls: Vec<&ToolCall> = bridged.iter().chain(completion.tool_calls.iter()).collect();
        let tool_calls_json = if all_calls.is_empty() { None } else { serde_json::to_value(&all_calls).ok() };
        let meta = json!({
            "alice_ms": alice_ms,
            "alice_us": alice_us,
            "ttft_ms": ttft,
            "provider_ms": provider_ms,
            "usage": completion.usage,
            "stop_reason": completion.stop_reason,
            "memories": mem_ids,
            // Last message of the turn. Calls made through MCP are already
            // answered, so `tool_calls` alone does not mean another step follows.
            "final": completion.tool_calls.is_empty(),
        });
        let updated = {
            let c = app.db.lock();
            let ok = storage::update_message(
                &c,
                &assistant.id,
                &MessageUpdate {
                    content: &completion.text,
                    tool_calls: tool_calls_json.as_ref(),
                    status: "complete",
                    provider_state: completion.provider_state.as_ref(),
                    meta: Some(&meta),
                },
            )
            .unwrap_or(false);
            if ok {
                let _ = memory::record_uses(&c, &assistant.id, &mem_ids);
                let _ = storage::touch_thread(&c, &user.thread_id);
            }
            ok.then(|| storage::get_message(&c, &assistant.id).ok().flatten()).flatten()
        };
        let Some(done) = updated else { return Outcome::Deleted };
        let _ = t_post;
        app.emit(json!({ "type": "message.completed", "thread_id": done.thread_id, "message": done }));

        if completion.tool_calls.is_empty() {
            crate::info!(
                "turn done thread={} steps={} total_ms={} provider_ms={} alice_ms={}",
                user.thread_id,
                step + 1,
                t_turn.elapsed().as_millis(),
                provider_ms_total,
                t_turn.elapsed().as_millis().saturating_sub(provider_ms_total)
            );
            return Outcome::Completed;
        }

        // Run tools.
        let source = if providers::supports_tools(provider.kind) { gate::Source::Native } else { gate::Source::Text };
        for call in &completion.tool_calls {
            let gc = gate::Call {
                thread_id: &user.thread_id,
                message_id: &assistant.id,
                call_id: &call.id,
                name: &call.name,
                args: &call.arguments,
                source,
                client: &user.client,
            };
            let (content, ok) = match gate::invoke(app, &gc, &cancel).await {
                gate::Outcome::Cancelled => return Outcome::Cancelled,
                gate::Outcome::Done(content, ok) => (content, ok),
            };
            let tool_msg = {
                let c = app.db.lock();
                let r = storage::insert_message(
                    &c,
                    &NewMessage {
                        thread_id: &user.thread_id,
                        role: "tool",
                        content: &content,
                        tool_calls: None,
                        tool_call_id: Some(&call.id),
                        status: "complete",
                        client: &user.client,
                        model: None,
                    },
                );
                match r {
                    Ok(m) => {
                        let meta = json!({ "tool": call.name, "ok": ok });
                        let _ = c.execute("UPDATE messages SET meta = ?2 WHERE id = ?1", rusqlite::params![m.id, meta.to_string()]);
                        Some(m)
                    }
                    Err(_) => None,
                }
            };
            if tool_msg.is_none() {
                return Outcome::Deleted;
            }
            app.emit(json!({
                "type": "tool.completed", "thread_id": user.thread_id, "message_id": assistant.id,
                "tool_call_id": call.id, "tool": call.name, "ok": ok,
                "summary": crate::util::truncate(&content, 300),
            }));
        }

        step += 1;
        if step >= cfg.agent.max_steps {
            let c = app.db.lock();
            let note = storage::insert_message(
                &c,
                &NewMessage {
                    thread_id: &user.thread_id,
                    role: "assistant",
                    content: "(Stopped: reached the maximum number of tool steps for one request.)",
                    tool_calls: None,
                    tool_call_id: None,
                    status: "complete",
                    client: &user.client,
                    model: Some(model_id),
                },
            );
            drop(c);
            return match note {
                Ok(m) => {
                    app.emit(json!({ "type": "message.created", "thread_id": m.thread_id, "message": m }));
                    app.emit(json!({ "type": "message.completed", "thread_id": m.thread_id, "message": m }));
                    Outcome::Completed
                }
                Err(_) => Outcome::Deleted,
            };
        }
        if cancel.is_cancelled() {
            return Outcome::Cancelled;
        }
        // Next step: a new assistant message.
        let next = {
            let c = app.db.lock();
            storage::insert_message(
                &c,
                &NewMessage {
                    thread_id: &user.thread_id,
                    role: "assistant",
                    content: "",
                    tool_calls: None,
                    tool_call_id: None,
                    status: "streaming",
                    client: &user.client,
                    model: Some(model_id),
                },
            )
        };
        match next {
            Ok(m) => {
                app.emit(json!({ "type": "message.created", "thread_id": m.thread_id, "message": m }));
                assistant = m;
            }
            Err(_) => return Outcome::Deleted,
        }
    }
}

fn finish_cancelled(app: &AppRef, m: &Message, partial: &str) {
    let c = app.db.lock();
    let ok = storage::update_message(
        &c,
        &m.id,
        &MessageUpdate { content: partial, tool_calls: None, status: "cancelled", provider_state: None, meta: None },
    )
    .unwrap_or(false);
    let msg = if ok { storage::get_message(&c, &m.id).ok().flatten() } else { None };
    drop(c);
    if let Some(msg) = msg {
        app.emit(json!({ "type": "generation.cancelled", "thread_id": m.thread_id, "message_id": m.id }));
        app.emit(json!({ "type": "message.completed", "thread_id": m.thread_id, "message": msg }));
    }
}

fn fail(app: &AppRef, m: &Message, partial: &str, err: &str) {
    warn!("generation failed thread={}: {}", m.thread_id, crate::util::truncate(err, 300));
    let c = app.db.lock();
    let meta = json!({ "error": err });
    let ok = storage::update_message(
        &c,
        &m.id,
        &MessageUpdate { content: partial, tool_calls: None, status: "error", provider_state: None, meta: Some(&meta) },
    )
    .unwrap_or(false);
    let msg = if ok { storage::get_message(&c, &m.id).ok().flatten() } else { None };
    drop(c);
    if let Some(msg) = msg {
        app.emit(json!({ "type": "error", "thread_id": m.thread_id, "message_id": m.id, "error": err }));
        app.emit(json!({ "type": "message.completed", "thread_id": m.thread_id, "message": msg }));
    }
}

/// Extract memories from the user's message (only user-authored text).
async fn extract_memories(app: &AppRef, user: &Message, thread_model: &str) {
    let cfg = app.config();
    if !cfg.memory.auto_extract || user.content.chars().count() < cfg.memory.min_chars {
        return;
    }
    let model_id = if cfg.memory.model.is_empty() { thread_model } else { cfg.memory.model.as_str() };
    let Some((provider, model_name)) = cfg.resolve_model(model_id) else { return };
    let provider = provider.clone();
    let api_key = app.secrets().provider_key(&provider.id).map(str::to_string);
    let (earlier, keys) = {
        let c = app.db.lock();
        let earlier: Vec<String> = c
            .prepare_cached(
                "SELECT content FROM messages WHERE thread_id = ?1 AND role = 'user' AND seq < ?2 ORDER BY seq DESC LIMIT 2",
            )
            .and_then(|mut st| st.query_map(rusqlite::params![user.thread_id, user.seq], |r| r.get(0))?.collect())
            .unwrap_or_default();
        let keys = memory::related_keys(&c, &user.content, 30).unwrap_or_default();
        (earlier.into_iter().rev().collect::<Vec<_>>(), keys)
    };
    let items = match memory::extract(&app.http, &provider, api_key.as_deref(), model_name, &user.content, &earlier, &keys).await {
        Ok(v) => v,
        Err(e) => {
            warn!("memory extraction failed: {}", crate::util::truncate(&e.0, 200));
            return;
        }
    };
    if items.is_empty() {
        return;
    }
    let touched = {
        let mut c = app.db.lock();
        memory::apply_extraction(&mut c, &user.id, &items)
    };
    match touched {
        Ok(ids) if !ids.is_empty() || items.iter().any(|i| i.forget) => {
            app.emit(json!({ "type": "memory.updated", "thread_id": user.thread_id, "memory_ids": ids }));
        }
        Ok(_) => {}
        Err(e) => error!("memory write failed: {e}"),
    }
}

/// Map a stored tool-calls value back into calls (used by clients/tests).
pub fn parse_tool_calls(v: &Value) -> Vec<ToolCall> {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_is_the_soul_plus_context() {
        let mut cfg = Config::default();
        cfg.agent.name = "Eve".into();
        let p = system_prompt(&cfg, "You are {{name}}.\nBe brief.", "<memories>\n- likes tea\n</memories>", Some(std::path::Path::new("/w")), "Skills:\n- files: x\n");
        assert!(p.starts_with("You are Eve.\nBe brief."), "{p}");
        assert!(p.contains("Current time (UTC):") && p.contains("Working folder for file tools: /w") && p.ends_with("</memories>\n"), "{p}");
        // Nothing else is added: an empty SOUL means no instructions at all.
        let bare = system_prompt(&cfg, "", "", None, "");
        assert!(bare.trim_start().starts_with("Current time") && bare.lines().count() <= 3, "{bare}");
    }
}
