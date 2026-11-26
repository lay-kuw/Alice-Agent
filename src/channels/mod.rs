//! Telegram / Discord gateways. Both run inside the server process and use
//! the same agent core as every other client. This module holds the shared
//! logic (commands, thread selection, streaming, approvals, deletion of
//! posted messages); `telegram.rs` / `discord.rs` only move bytes.

pub mod discord;
pub mod telegram;

use crate::agent::{self, TurnInput};
use crate::app::AppRef;
use crate::config::{Config, Paths, Secrets};
use crate::storage;
use crate::util::now_ms;
use crate::{info, warn};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct Button {
    pub label: String,
    pub data: String,
    pub danger: bool,
}

fn btn(label: impl Into<String>, data: impl Into<String>) -> Button {
    Button { label: label.into(), data: data.into(), danger: false }
}

/// What a chat platform must provide.
pub trait Surface: Send + Sync + 'static {
    fn channel(&self) -> &'static str;
    fn max_len(&self) -> usize;
    /// Send a message; returns its platform ID.
    fn send(&self, chat: &str, text: &str, buttons: &[Button]) -> impl Future<Output = Result<String, String>> + Send;
    fn edit(&self, chat: &str, id: &str, text: &str, buttons: &[Button]) -> impl Future<Output = Result<(), String>> + Send;
    fn delete(&self, chat: &str, id: &str) -> impl Future<Output = Result<(), String>> + Send;
    fn typing(&self, chat: &str) -> impl Future<Output = ()> + Send;
}

pub struct Incoming {
    pub chat_id: String,
    pub message_id: String,
    pub text: String,
    pub files: Vec<(String, String, Vec<u8>)>, // name, mime, bytes
}

// ------------------------------------------------------------------ state

fn chat_state(app: &AppRef, channel: &str, chat: &str) -> (Option<String>, Option<String>) {
    let c = app.db.lock();
    c.query_row(
        "SELECT thread_id, model FROM channel_state WHERE channel = ?1 AND chat_id = ?2",
        params![channel, chat],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or((None, None))
}

fn set_thread(app: &AppRef, channel: &str, chat: &str, thread: Option<&str>) {
    let c = app.db.lock();
    let _ = c.execute(
        "INSERT INTO channel_state (channel, chat_id, thread_id) VALUES (?1, ?2, ?3)
         ON CONFLICT(channel, chat_id) DO UPDATE SET thread_id = excluded.thread_id",
        params![channel, chat, thread],
    );
}

fn record_external(app: &AppRef, thread: &str, channel: &str, chat: &str, id: &str) {
    let c = app.db.lock();
    let _ = c.execute(
        "INSERT INTO channel_messages (thread_id, channel, chat_id, external_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![thread, channel, chat, id, now_ms()],
    );
}

/// Messages posted on external platforms for a thread (read before deleting it).
pub fn external_messages(c: &rusqlite::Connection, thread: Option<&str>) -> Vec<Value> {
    let map = |r: &rusqlite::Row| -> rusqlite::Result<Value> {
        Ok(json!({ "channel": r.get::<_, String>(0)?, "chat_id": r.get::<_, String>(1)?, "external_id": r.get::<_, String>(2)? }))
    };
    let rows = match thread {
        Some(t) => c
            .prepare("SELECT channel, chat_id, external_id FROM channel_messages WHERE thread_id = ?1")
            .and_then(|mut st| st.query_map([t], map)?.collect::<rusqlite::Result<Vec<_>>>()),
        None => c
            .prepare("SELECT channel, chat_id, external_id FROM channel_messages LIMIT 5000")
            .and_then(|mut st| st.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()),
    };
    rows.unwrap_or_default()
}

fn ensure_thread(app: &AppRef, channel: &str, chat: &str) -> Result<String, String> {
    let (cur, _) = chat_state(app, channel, chat);
    if let Some(t) = cur
        && storage::get_thread(&app.db.lock(), &t).ok().flatten().is_some() {
            return Ok(t);
        }
    let t = storage::create_thread(&app.db.lock(), "", None).map_err(|e| e.to_string())?;
    app.emit(json!({ "type": "thread.created", "thread_id": t.id, "thread": t }));
    set_thread(app, channel, chat, Some(&t.id));
    Ok(t.id)
}

// ------------------------------------------------------------------ formatting

/// Split long text at line boundaries.
pub fn split(text: &str, max: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.split_inclusive('\n') {
        if cur.len() + line.len() > max && !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
        let mut l = line;
        while l.len() > max {
            let cut = crate::util::truncate(l, max);
            parts.push(cut.to_string());
            l = &l[cut.len()..];
        }
        cur.push_str(l);
    }
    if !cur.trim().is_empty() || parts.is_empty() {
        parts.push(cur);
    }
    parts
}

/// Streaming preview: the tail of the text plus a cursor, within `max` bytes.
fn preview(text: &str, max: usize) -> String {
    let t = if text.is_empty() { "…" } else { text };
    if t.len() + 4 <= max {
        return format!("{t} ▍");
    }
    let mut start = t.len() - (max - 8);
    while !t.is_char_boundary(start) {
        start += 1;
    }
    format!("…{} ▍", &t[start..])
}

// ------------------------------------------------------------------ handling

const HELP: &str = "Alice commands:
/new [title] — start a new thread
/threads — list recent threads
/use <id> — switch to a thread
/model [id] — show or change this thread's model
/rename <title> — rename the current thread
/delete — delete the current thread (and memories from it)
/stop — stop the current response
/status — current thread, model and server status
/usage — subscription limits and tokens used
Anything else is sent to Alice.";

/// Entry point for a message from an allowed user.
pub async fn handle_message<S: Surface>(app: &AppRef, s: &Arc<S>, msg: Incoming) {
    let ch = s.channel();
    let text = msg.text.trim().to_string();
    if text.starts_with('/') && msg.files.is_empty() {
        let reply = command(app, s, &msg.chat_id, &text).await;
        if let Some((text, buttons)) = reply {
            let _ = s.send(&msg.chat_id, &text, &buttons).await;
        }
        return;
    }
    let thread = match ensure_thread(app, ch, &msg.chat_id) {
        Ok(t) => t,
        Err(e) => {
            let _ = s.send(&msg.chat_id, &format!("Error: {e}"), &[]).await;
            return;
        }
    };
    record_external(app, &thread, ch, &msg.chat_id, &msg.message_id);
    let mut attachment_ids = Vec::new();
    for (name, mime, bytes) in msg.files {
        match save_attachment(app, &thread, &name, &mime, &bytes) {
            Ok(id) => attachment_ids.push(id),
            Err(e) => warn!("{ch}: attachment failed: {e}"),
        }
    }
    let (_, chat_model) = chat_state(app, ch, &msg.chat_id);
    run_turn(app, s, &msg.chat_id, &thread, text, chat_model, attachment_ids).await;
}

pub fn save_attachment(app: &AppRef, thread: &str, name: &str, mime: &str, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 25 * 1024 * 1024 {
        return Err("file too large".into());
    }
    let id = crate::util::new_id();
    let dir = app.paths.attachments().join(thread);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(&id), bytes).map_err(|e| e.to_string())?;
    let name: String = name.chars().filter(|c| !c.is_control() && *c != '/' && *c != '\\').take(200).collect();
    let mime = if mime.contains('/') { mime.to_string() } else { crate::server::guess_mime(&name).to_string() };
    let r = app.db.lock().execute(
        "INSERT INTO attachments (id, thread_id, name, mime, size, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, thread, name, mime, bytes.len() as i64, now_ms()],
    );
    if let Err(e) = r {
        let _ = std::fs::remove_file(dir.join(&id));
        return Err(e.to_string());
    }
    Ok(id)
}

async fn run_turn<S: Surface>(
    app: &AppRef,
    s: &Arc<S>,
    chat: &str,
    thread: &str,
    text: String,
    model: Option<String>,
    attachments: Vec<String>,
) {
    let ch = s.channel();
    let mut rx = app.events.subscribe();
    let started = agent::start_turn(
        app,
        TurnInput { thread_id: thread.to_string(), text, client: ch.into(), model, attachment_ids: attachments, after: None },
    );
    if let Err(e) = started {
        let _ = s.send(chat, &format!("⚠️ {e}"), &[]).await;
        return;
    }
    s.typing(chat).await;
    let max = s.max_len();
    let mut text = String::new();
    let mut tools: Vec<String> = Vec::new();
    let mut msg_id: Option<String> = None;
    let mut last_edit = Instant::now() - Duration::from_secs(5);
    let mut dirty = false;
    let tick = tokio::time::sleep(Duration::from_millis(1200));
    tokio::pin!(tick);
    let render = |text: &str, tools: &[String]| {
        let mut out = String::new();
        for t in tools {
            out.push_str(t);
            out.push('\n');
        }
        if !tools.is_empty() && !text.is_empty() {
            out.push('\n');
        }
        out.push_str(text);
        out
    };
    loop {
        let ev = tokio::select! {
            r = rx.recv() => match r {
                Ok(ev) => ev,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            },
            _ = &mut tick, if dirty => {
                // Throttled streaming update.
                let body = preview(&render(&text, &tools), max);
                match &msg_id {
                    Some(id) => { let _ = s.edit(chat, id, &body, &[]).await; }
                    None => if let Ok(id) = s.send(chat, &body, &[]).await {
                        record_external(app, thread, ch, chat, &id);
                        msg_id = Some(id);
                    },
                }
                last_edit = Instant::now();
                dirty = false;
                continue;
            }
        };
        if ev.thread_id.as_deref() != Some(thread) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&ev.json) else { continue };
        match v["type"].as_str().unwrap_or("") {
            "message.delta" => {
                text.push_str(v["delta"].as_str().unwrap_or(""));
                if !dirty {
                    dirty = true;
                    let wait = Duration::from_millis(1200).saturating_sub(last_edit.elapsed());
                    tick.as_mut().reset(tokio::time::Instant::now() + wait);
                }
            }
            "tool.started" => {
                tools.push(format!("⚙ {}", v["tool"].as_str().unwrap_or("")));
                dirty = true;
                tick.as_mut().reset(tokio::time::Instant::now());
            }
            "tool.completed" if v["ok"] == false => {
                if let Some(last) = tools.last_mut() {
                    last.push_str(" ✗");
                }
            }
            "tool.approval_required" => {
                let id = v["approval_id"].as_str().unwrap_or("");
                let args = serde_json::to_string_pretty(&v["arguments"]).unwrap_or_default();
                let body = format!("Allow {}?\n```\n{}\n```", v["tool"].as_str().unwrap_or(""), crate::util::truncate(&args, 1200));
                let buttons = [btn("Allow", format!("ap:{id}:y")), Button { label: "Deny".into(), data: format!("ap:{id}:n"), danger: true }];
                if let Ok(mid) = s.send(chat, &body, &buttons).await {
                    record_external(app, thread, ch, chat, &mid);
                }
            }
            "message.completed" if v["message"]["role"] == "assistant" => {
                let m = &v["message"];
                let has_tools = m["tool_calls"].as_array().is_some_and(|a| !a.is_empty()) && m["meta"]["final"] != true;
                let status = m["status"].as_str().unwrap_or("");
                if status == "complete" && has_tools {
                    // Keep the visible text; the next step streams after it.
                    if !text.is_empty() && !text.ends_with('\n') {
                        text.push_str("\n\n");
                    }
                    continue;
                }
                let mut final_text = render(&text, &tools);
                match status {
                    "cancelled" => final_text.push_str("\n(stopped)"),
                    "error" => final_text = format!("{final_text}\n⚠️ {}", m.pointer("/meta/error").and_then(|e| e.as_str()).unwrap_or("generation failed")),
                    _ => {}
                }
                let parts = split(final_text.trim(), max);
                let mut first = true;
                for p in parts {
                    match (&msg_id, first) {
                        (Some(id), true) => {
                            let _ = s.edit(chat, id, &p, &[]).await;
                        }
                        _ => {
                            if let Ok(id) = s.send(chat, &p, &[]).await {
                                record_external(app, thread, ch, chat, &id);
                            }
                        }
                    }
                    first = false;
                }
                return;
            }
            "thread.deleted" => return,
            _ => {}
        }
    }
}

async fn command<S: Surface>(app: &AppRef, s: &Arc<S>, chat: &str, text: &str) -> Option<(String, Vec<Button>)> {
    let ch = s.channel();
    let (cmd, arg) = text.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((text, ""));
    // Telegram appends @botname in groups.
    let cmd = cmd.split('@').next().unwrap_or(cmd);
    let (cur, _) = chat_state(app, ch, chat);
    let cur = cur.filter(|t| storage::get_thread(&app.db.lock(), t).ok().flatten().is_some());
    Some(match cmd {
        "/start" | "/help" => (HELP.into(), vec![]),
        "/new" => {
            let t = match storage::create_thread(&app.db.lock(), arg, None) {
                Ok(t) => t,
                Err(e) => return Some((format!("Error: {e}"), vec![])),
            };
            app.emit(json!({ "type": "thread.created", "thread_id": t.id, "thread": t }));
            set_thread(app, ch, chat, Some(&t.id));
            (format!("New thread {}. Send a message to start.", t.id), vec![])
        }
        "/threads" | "/list" => {
            let threads = storage::list_threads(&app.db.lock()).unwrap_or_default();
            if threads.is_empty() {
                return Some(("No threads yet.".into(), vec![]));
            }
            let buttons = threads
                .iter()
                .take(10)
                .map(|t| {
                    let title = if t.title.is_empty() { "(untitled)" } else { t.title.as_str() };
                    let mark = if Some(&t.id) == cur.as_ref() { "• " } else { "" };
                    btn(format!("{mark}{}", crate::util::truncate(title, 40)), format!("th:{}", t.id))
                })
                .collect();
            ("Recent threads (shared with all your devices):".into(), buttons)
        }
        "/use" | "/switch" => {
            let id = storage::resolve_thread_id(&app.db.lock(), arg).ok().flatten();
            match id {
                Some(id) => {
                    set_thread(app, ch, chat, Some(&id));
                    (format!("Switched to {}.", thread_label(app, &id)), vec![])
                }
                None => ("No such thread. Use /threads.".into(), vec![]),
            }
        }
        "/model" => {
            let cfg = app.config();
            let models = cfg.all_models();
            let thread_model = cur.as_ref().and_then(|t| storage::get_thread(&app.db.lock(), t).ok().flatten()).and_then(|t| t.model);
            let current = thread_model.clone().unwrap_or(cfg.agent.default_model.clone());
            if arg.is_empty() {
                let buttons = models
                    .iter()
                    .enumerate()
                    .map(|(i, m)| btn(if *m == current { format!("• {m}") } else { m.clone() }, format!("md:{i}")))
                    .collect();
                (format!("Model: {current}"), buttons)
            } else {
                set_model(app, ch, chat, cur.as_deref(), arg)
            }
        }
        "/rename" => match (&cur, arg.is_empty()) {
            (Some(t), false) => {
                if let Ok(Some(th)) = storage::update_thread(&app.db.lock(), t, Some(arg), None) {
                    app.emit(json!({ "type": "thread.updated", "thread_id": th.id, "thread": th }));
                }
                ("Renamed.".into(), vec![])
            }
            _ => ("Usage: /rename <title> (in a thread)".into(), vec![]),
        },
        "/delete" => match &cur {
            Some(t) => (
                format!("Delete {}? Memories learned from it are deleted too.", thread_label(app, t)),
                vec![Button { label: "Delete".into(), data: format!("dl:{t}"), danger: true }, btn("Cancel", "cancel")],
            ),
            None => ("No current thread.".into(), vec![]),
        },
        "/stop" => match &cur {
            Some(t) if app.cancel_generation(t) => ("Stopping…".into(), vec![]),
            _ => ("Nothing is running.".into(), vec![]),
        },
        "/status" => {
            let cfg = app.config();
            let counts = storage::counts(&app.db.lock()).ok();
            let thread = cur.as_ref().map(|t| thread_label(app, t)).unwrap_or("(none — a new one starts with your next message)".into());
            (
                format!(
                    "Alice {} (profile {})\nThread: {}\nDefault model: {}\nThreads: {}  Memories: {}",
                    env!("CARGO_PKG_VERSION"),
                    app.paths.profile,
                    thread,
                    cfg.agent.default_model,
                    counts.as_ref().map(|c| c.threads).unwrap_or(0),
                    counts.as_ref().map(|c| c.memories).unwrap_or(0)
                ),
                vec![],
            )
        }
        "/usage" => (crate::usage::render_text(&crate::usage::collect(app).await), vec![]),
        _ => ("Unknown command. /help".into(), vec![]),
    })
}

fn thread_label(app: &AppRef, id: &str) -> String {
    match storage::get_thread(&app.db.lock(), id).ok().flatten() {
        Some(t) if !t.title.is_empty() => format!("“{}” ({})", t.title, t.id),
        _ => id.to_string(),
    }
}

fn set_model(app: &AppRef, ch: &str, chat: &str, thread: Option<&str>, model: &str) -> (String, Vec<Button>) {
    if app.config().resolve_model(model).is_none() {
        return (format!("Unknown model: {model}"), vec![]);
    }
    match thread {
        Some(t) => {
            if let Ok(Some(th)) = storage::update_thread(&app.db.lock(), t, None, Some(Some(model))) {
                app.emit(json!({ "type": "thread.updated", "thread_id": th.id, "thread": th }));
            }
        }
        None => {
            let c = app.db.lock();
            let _ = c.execute(
                "INSERT INTO channel_state (channel, chat_id, model) VALUES (?1, ?2, ?3)
                 ON CONFLICT(channel, chat_id) DO UPDATE SET model = excluded.model",
                params![ch, chat, model],
            );
        }
    }
    (format!("Model: {model}"), vec![])
}

/// A button press from an allowed user. Returns a short status text.
pub async fn handle_button<S: Surface>(app: &AppRef, s: &Arc<S>, chat: &str, data: &str) -> String {
    let ch = s.channel();
    if let Some(rest) = data.strip_prefix("ap:") {
        let (id, ans) = rest.rsplit_once(':').unwrap_or((rest, "n"));
        let p = app.approvals.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        return match p {
            Some(p) => {
                let _ = p.reply.send(ans == "y");
                if ans == "y" { "Allowed".into() } else { "Denied".into() }
            }
            None => "This request is no longer pending.".into(),
        };
    }
    if let Some(id) = data.strip_prefix("th:") {
        if storage::get_thread(&app.db.lock(), id).ok().flatten().is_some() {
            set_thread(app, ch, chat, Some(id));
            return format!("Switched to {}.", thread_label(app, id));
        }
        return "That thread no longer exists.".into();
    }
    if let Some(i) = data.strip_prefix("md:") {
        let models = app.config().all_models();
        let Some(m) = i.parse::<usize>().ok().and_then(|i| models.get(i).cloned()) else { return "Unknown model.".into() };
        let (cur, _) = chat_state(app, ch, chat);
        return set_model(app, ch, chat, cur.as_deref(), &m).0;
    }
    if let Some(id) = data.strip_prefix("dl:") {
        let app2 = app.clone();
        let id2 = id.to_string();
        let r = tokio::task::spawn_blocking(move || crate::server::delete_thread_now(&app2, &id2)).await;
        return match r {
            Ok(Ok(true)) => "Thread deleted.".into(),
            _ => "Thread not found.".into(),
        };
    }
    "OK".into()
}

/// Best-effort removal of posted messages after a thread is deleted.
pub async fn delete_external<S: Surface>(s: &Arc<S>, v: &Value) {
    let Some(list) = v["external"].as_array() else { return };
    for m in list {
        if m["channel"] != s.channel() {
            continue;
        }
        let (chat, id) = (m["chat_id"].as_str().unwrap_or(""), m["external_id"].as_str().unwrap_or(""));
        if let Err(e) = s.delete(chat, id).await {
            // Telegram cannot delete messages older than 48h; this is outside Alice's guarantee.
            info!("{}: could not delete message: {}", s.channel(), crate::util::truncate(&e, 120));
        }
    }
}

// ------------------------------------------------------------------ supervisor

/// Run each enabled gateway; restart it when the configuration changes.
pub fn start(app: &AppRef) {
    let a = app.clone();
    tokio::spawn(supervise(a, "telegram"));
    let a = app.clone();
    tokio::spawn(supervise(a, "discord"));
}

fn gateway_ready(cfg: &Config, secrets: &Secrets, name: &str) -> Option<String> {
    let (enabled, key) = match name {
        "telegram" => (cfg.channels.telegram.enabled, "telegram.bot_token"),
        _ => (cfg.channels.discord.enabled, "discord.bot_token"),
    };
    if !enabled {
        return None;
    }
    secrets.get(key).map(str::to_string)
}

async fn supervise(app: AppRef, name: &'static str) {
    let mut rx = app.events.subscribe();
    loop {
        let cfg = app.config();
        let token = gateway_ready(&cfg, &app.secrets(), name);
        let has_token = token.is_some();
        let stop = CancellationToken::new();
        let run = {
            let app = app.clone();
            let stop = stop.clone();
            async move {
                match token {
                    Some(t) => {
                        info!("{name}: gateway starting");
                        if name == "telegram" {
                            telegram::run(app, t, stop).await
                        } else {
                            discord::run(app, t, stop).await
                        }
                    }
                    None => std::future::pending().await,
                }
            }
        };
        if !has_token {
            let cfg = app.config();
            let enabled = if name == "telegram" { cfg.channels.telegram.enabled } else { cfg.channels.discord.enabled };
            if enabled {
                app.set_channel_status(name, "error", "bot token is not set");
            } else {
                app.set_channel_status(name, "disabled", "");
            }
        } else {
            app.set_channel_status(name, "connecting", "");
        }
        let fingerprint = gateway_fingerprint(&app, name);
        tokio::pin!(run);
        loop {
            tokio::select! {
                _ = &mut run => {
                    // Gateway exited (fatal error); retry later.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    break;
                }
                ev = rx.recv() => {
                    if let Ok(ev) = ev
                        && ev.json.contains("\"config.updated\"") && gateway_fingerprint(&app, name) != fingerprint {
                            stop.cancel();
                            info!("{name}: configuration changed; restarting gateway");
                            break;
                        }
                }
            }
        }
        stop.cancel();
    }
}

/// Changes to this value restart the gateway (kept in memory only).
fn gateway_fingerprint(app: &AppRef, name: &str) -> String {
    let cfg = app.config();
    let secrets = app.secrets();
    match name {
        "telegram" => format!("{:?}{:?}", cfg.channels.telegram, secrets.get("telegram.bot_token")),
        _ => format!("{:?}{:?}", cfg.channels.discord, secrets.get("discord.bot_token")),
    }
}

// ------------------------------------------------------------------ CLI

pub async fn gateway_cmd(paths: &Paths, args: Vec<String>) -> Result<(), String> {
    use crate::cli::{ask, ask_secret, client::Client};
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let remote = match Client::from_env(paths) {
        Ok(c) if c.is_up().await => Some(c),
        _ => None,
    };
    let cfg = match &remote {
        Some(c) => serde_json::from_value::<Config>(c.get("/config").await?).map_err(|e| e.to_string())?,
        None => Config::load(paths)?,
    };
    let secrets_set: Vec<String> = match &remote {
        Some(c) => c.get("/config").await?["secrets_set"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        None => Secrets::load(paths)?.names().cloned().collect(),
    };
    let apply = |patch: Value, secret: Option<(&'static str, String)>| {
        let remote = remote.clone();
        async move {
            match remote {
                Some(c) => {
                    if let Some((name, v)) = &secret {
                        if v.is_empty() {
                            c.delete(&format!("/secrets/{name}"), None).await?;
                        } else {
                            c.put(&format!("/secrets/{name}"), &json!({ "value": v })).await?;
                        }
                    }
                    c.patch("/config", &patch).await?;
                }
                None => {
                    let mut cfg = Config::load(paths)?;
                    for (k, v) in patch.as_object().unwrap() {
                        let raw = match v {
                            Value::String(s) => s.clone(),
                            o => o.to_string(),
                        };
                        cfg.set_key(k, &raw)?;
                    }
                    cfg.save(paths)?;
                    if let Some((name, v)) = secret {
                        let mut s = Secrets::load(paths)?;
                        s.set(name, &v);
                        s.save(paths)?;
                    }
                }
            }
            Ok::<(), String>(())
        }
    };
    match a.as_slice() {
        [] | ["list" | "ls"] => {
            let tg = &cfg.channels.telegram;
            let dc = &cfg.channels.discord;
            let has = |n: &str| secrets_set.iter().any(|s| s == n);
            println!(
                "telegram  {}  token {}  allowed users {:?}",
                if tg.enabled { "enabled " } else { "disabled" },
                if has("telegram.bot_token") { "set" } else { "missing" },
                tg.allowed_user_ids
            );
            println!(
                "discord   {}  token {}  allowed users {:?}",
                if dc.enabled { "enabled " } else { "disabled" },
                if has("discord.bot_token") { "set" } else { "missing" },
                dc.allowed_user_ids
            );
            Ok(())
        }
        ["setup"] | ["setup", _] => {
            let kind = match a.get(1) {
                Some(k) => k.to_string(),
                None => ask("Gateway (telegram / discord)", "telegram"),
            };
            match kind.as_str() {
                "telegram" => {
                    println!("Create a bot with @BotFather and paste its token. Find your numeric user ID with @userinfobot.");
                    let token = ask_secret("Bot token (input hidden)");
                    let ids = ask("Allowed Telegram user IDs (comma separated)", &cfg.channels.telegram.allowed_user_ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","));
                    let ids: Vec<i64> = ids.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                    if ids.is_empty() {
                        return Err("at least one allowed user ID is required (everyone else is rejected)".into());
                    }
                    let secret = if token.is_empty() { None } else { Some(("telegram.bot_token", token)) };
                    apply(json!({ "channels.telegram.allowed_user_ids": ids, "channels.telegram.enabled": true }), secret).await?;
                }
                "discord" => {
                    println!("Create an application + bot in the Discord Developer Portal, invite it, and paste the bot token.");
                    println!("Enable Developer Mode in Discord to copy your user ID. DMs work out of the box; in servers, mention the bot.");
                    let token = ask_secret("Bot token (input hidden)");
                    let ids = ask("Allowed Discord user IDs (comma separated)", &cfg.channels.discord.allowed_user_ids.join(","));
                    let ids: Vec<String> = ids.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).collect();
                    if ids.is_empty() {
                        return Err("at least one allowed user ID is required (everyone else is rejected)".into());
                    }
                    let secret = if token.is_empty() { None } else { Some(("discord.bot_token", token)) };
                    apply(json!({ "channels.discord.allowed_user_ids": ids, "channels.discord.enabled": true }), secret).await?;
                }
                other => return Err(format!("unknown gateway: {other}")),
            }
            println!("Gateway {kind} enabled{}.", if remote.is_some() { " (applied to the running server)" } else { "; it starts with the server" });
            Ok(())
        }
        ["delete" | "rm", id] => {
            let (key, secret) = match *id {
                "telegram" => ("channels.telegram.enabled", "telegram.bot_token"),
                "discord" => ("channels.discord.enabled", "discord.bot_token"),
                other => return Err(format!("unknown gateway: {other} (telegram or discord)")),
            };
            apply(json!({ key: false }), Some((secret, String::new()))).await?;
            println!("Gateway {id} disabled and its token removed.");
            Ok(())
        }
        _ => Err("usage: alice gateway [list | setup [telegram|discord] | delete <telegram|discord>]".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting() {
        let parts = split(&"line\n".repeat(100), 50);
        assert!(parts.iter().all(|p| p.len() <= 50));
        assert_eq!(parts.concat(), "line\n".repeat(100));
        let long = "あ".repeat(100);
        assert!(split(&long, 31).iter().all(|p| p.len() <= 31));
        assert!(preview(&"x".repeat(200), 50).len() <= 50);
    }
}
