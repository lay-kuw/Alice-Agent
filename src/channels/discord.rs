//! Discord gateway: Gateway WebSocket for events, REST for messages.
//! Works in DMs, and in servers when the bot is mentioned. Text commands
//! (`/new`, `/model`, …) and button components for approvals.

use super::{Button, Incoming, Surface};
use crate::app::AppRef;
use crate::{info, warn};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_util::sync::CancellationToken;

const MAX_FILE: u64 = 20 * 1024 * 1024;
// GUILD_MESSAGES | DIRECT_MESSAGES. Content of guild messages is delivered
// when the bot is mentioned; DMs always include content.
const INTENTS: u64 = (1 << 9) | (1 << 12);

pub struct Discord {
    http: reqwest::Client,
    base: String,
    token: String,
    bot_id: std::sync::Mutex<String>,
}

impl Discord {
    pub fn new(app: &AppRef, token: &str) -> Discord {
        let api = app.config().channels.discord.api_base;
        Discord {
            http: reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("http"),
            base: if api.is_empty() { "https://discord.com/api/v10".into() } else { api.trim_end_matches('/').to_string() },
            token: token.to_string(),
            bot_id: std::sync::Mutex::new(String::new()),
        }
    }

    pub async fn rest(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value, String> {
        for _ in 0..3 {
            let mut rb = self.http.request(method.clone(), format!("{}{path}", self.base)).header("Authorization", format!("Bot {}", self.token));
            if let Some(b) = body {
                rb = rb.json(b);
            }
            let r = rb.send().await.map_err(|e| e.without_url().to_string())?;
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            if status.as_u16() == 429 {
                let wait = v["retry_after"].as_f64().unwrap_or(1.0).min(30.0);
                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                continue;
            }
            if !status.is_success() {
                return Err(format!("HTTP {}: {}", status.as_u16(), v["message"].as_str().unwrap_or("")));
            }
            return Ok(v);
        }
        Err("rate limited".into())
    }

    fn components(buttons: &[Button]) -> Value {
        let rows: Vec<Value> = buttons
            .chunks(5)
            .take(5)
            .map(|row| {
                json!({ "type": 1, "components": row.iter().map(|b| json!({
                    "type": 2, "style": if b.danger { 4 } else { 1 },
                    "label": crate::util::truncate(&b.label, 80), "custom_id": b.data,
                })).collect::<Vec<_>>() })
            })
            .collect();
        Value::Array(rows)
    }
}

impl Surface for Discord {
    fn channel(&self) -> &'static str {
        "discord"
    }
    fn max_len(&self) -> usize {
        1990
    }
    async fn send(&self, chat: &str, text: &str, buttons: &[Button]) -> Result<String, String> {
        let body = json!({ "content": text, "components": Self::components(buttons), "allowed_mentions": { "parse": [] } });
        let v = self.rest(reqwest::Method::POST, &format!("/channels/{chat}/messages"), Some(&body)).await?;
        Ok(v["id"].as_str().unwrap_or("").to_string())
    }
    async fn edit(&self, chat: &str, id: &str, text: &str, buttons: &[Button]) -> Result<(), String> {
        let body = json!({ "content": text, "components": Self::components(buttons), "allowed_mentions": { "parse": [] } });
        self.rest(reqwest::Method::PATCH, &format!("/channels/{chat}/messages/{id}"), Some(&body)).await.map(|_| ())
    }
    async fn delete(&self, chat: &str, id: &str) -> Result<(), String> {
        self.rest(reqwest::Method::DELETE, &format!("/channels/{chat}/messages/{id}"), None).await.map(|_| ())
    }
    async fn typing(&self, chat: &str) {
        let _ = self.rest(reqwest::Method::POST, &format!("/channels/{chat}/typing"), None).await;
    }
}

fn allowed(app: &AppRef, user: &str) -> bool {
    !user.is_empty() && app.config().channels.discord.allowed_user_ids.iter().any(|u| u == user)
}

/// Handle one gateway dispatch. Public for tests.
pub async fn dispatch(app: &AppRef, dc: &Arc<Discord>, t: &str, d: &Value) {
    match t {
        "READY" => {
            let id = d.pointer("/user/id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            info!("discord: connected as {}", d.pointer("/user/username").and_then(|u| u.as_str()).unwrap_or("?"));
            *dc.bot_id.lock().unwrap_or_else(|e| e.into_inner()) = id;
        }
        "MESSAGE_CREATE" => {
            let author = d.pointer("/author/id").and_then(|i| i.as_str()).unwrap_or("");
            if d.pointer("/author/bot").and_then(|b| b.as_bool()) == Some(true) {
                return;
            }
            if !allowed(app, author) {
                if d.get("guild_id").is_none() {
                    warn!("discord: ignored DM from user {author} (not in allowlist)");
                }
                return;
            }
            let bot = dc.bot_id.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let mut text = d["content"].as_str().unwrap_or("").to_string();
            if d.get("guild_id").is_some_and(|g| !g.is_null()) {
                let mentioned = d["mentions"].as_array().is_some_and(|m| m.iter().any(|u| u["id"] == bot.as_str()));
                if !mentioned {
                    return;
                }
                text = text.replace(&format!("<@{bot}>"), "").replace(&format!("<@!{bot}>"), "").trim().to_string();
            }
            let chat = d["channel_id"].as_str().unwrap_or("").to_string();
            let mut files = Vec::new();
            for a in d["attachments"].as_array().cloned().unwrap_or_default() {
                if a["size"].as_u64().unwrap_or(0) > MAX_FILE {
                    continue;
                }
                let Some(url) = a["url"].as_str() else { continue };
                if let Ok(r) = dc.http.get(url).send().await
                    && let Ok(b) = r.bytes().await {
                        files.push((
                            a["filename"].as_str().unwrap_or("file").to_string(),
                            a["content_type"].as_str().unwrap_or("").to_string(),
                            b.to_vec(),
                        ));
                    }
            }
            if text.is_empty() && files.is_empty() {
                return;
            }
            let msg = Incoming { chat_id: chat, message_id: d["id"].as_str().unwrap_or("").to_string(), text, files };
            super::handle_message(app, dc, msg).await;
        }
        "INTERACTION_CREATE" => {
            if d["type"].as_u64() != Some(3) {
                return;
            }
            let user = d.pointer("/member/user/id").or_else(|| d.pointer("/user/id")).and_then(|i| i.as_str()).unwrap_or("");
            let (id, token) = (d["id"].as_str().unwrap_or(""), d["token"].as_str().unwrap_or(""));
            if !allowed(app, user) {
                let _ = dc
                    .rest(reqwest::Method::POST, &format!("/interactions/{id}/{token}/callback"), Some(&json!({ "type": 4, "data": { "content": "Not allowed.", "flags": 64 } })))
                    .await;
                return;
            }
            let chat = d["channel_id"].as_str().unwrap_or("");
            let data = d.pointer("/data/custom_id").and_then(|c| c.as_str()).unwrap_or("");
            let status = super::handle_button(app, dc, chat, data).await;
            let old = d.pointer("/message/content").and_then(|c| c.as_str()).unwrap_or("");
            // Update the message: show the outcome, remove the buttons.
            let _ = dc
                .rest(
                    reqwest::Method::POST,
                    &format!("/interactions/{id}/{token}/callback"),
                    Some(&json!({ "type": 7, "data": { "content": format!("{old}\n→ {status}"), "components": [] } })),
                )
                .await;
        }
        _ => {}
    }
}

pub async fn run(app: AppRef, token: String, stop: CancellationToken) {
    let dc = Arc::new(Discord::new(&app, &token));
    {
        let dc = dc.clone();
        let mut rx = app.events.subscribe();
        let stop = stop.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.cancelled() => return,
                    ev = rx.recv() => match ev {
                        Ok(ev) if ev.json.contains("\"thread.deleted\"") || ev.json.contains("\"threads.cleared\"") => {
                            if let Ok(v) = serde_json::from_str::<Value>(&ev.json) {
                                super::delete_external(&dc, &v).await;
                            }
                        }
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => return,
                    }
                }
            }
        });
    }
    let mut backoff = 1u64;
    loop {
        match session(&app, &dc, &stop).await {
            Ok(()) => return, // stopped
            Err(e) => {
                warn!("discord: gateway disconnected: {}", crate::util::truncate(&e, 200));
                if e.contains("4004") || e.contains("4014") {
                    warn!("discord: authentication failed or intents not allowed; check the bot token");
                    return;
                }
            }
        }
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
        }
        backoff = (backoff * 2).min(60);
    }
}

/// One gateway connection. Returns Ok(()) only when asked to stop.
async fn session(app: &AppRef, dc: &Arc<Discord>, stop: &CancellationToken) -> Result<(), String> {
    let g = dc.rest(reqwest::Method::GET, "/gateway/bot", None).await?;
    let url = format!("{}/?v=10&encoding=json", g["url"].as_str().unwrap_or("wss://gateway.discord.gg"));
    let (mut ws, _) = tokio_tungstenite::connect_async(url.as_str()).await.map_err(|e| e.to_string())?;
    let mut seq: Value = Value::Null;
    let mut interval = Duration::from_secs(41);
    let mut beat = tokio::time::interval(interval);
    beat.tick().await;
    loop {
        tokio::select! {
            _ = stop.cancelled() => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            _ = beat.tick() => {
                ws.send(Ws::text(json!({ "op": 1, "d": seq }).to_string())).await.map_err(|e| e.to_string())?;
            }
            msg = ws.next() => {
                let msg = msg.ok_or("connection closed")?.map_err(|e| e.to_string())?;
                let text = match msg {
                    Ws::Text(t) => t.to_string(),
                    Ws::Close(f) => return Err(format!("closed: {}", f.map(|f| format!("{} {}", u16::from(f.code), f.reason)).unwrap_or_default())),
                    _ => continue,
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                if !v["s"].is_null() {
                    seq = v["s"].clone();
                }
                match v["op"].as_u64().unwrap_or(99) {
                    10 => {
                        interval = Duration::from_millis(v.pointer("/d/heartbeat_interval").and_then(|n| n.as_u64()).unwrap_or(41_250));
                        beat = tokio::time::interval(interval);
                        beat.tick().await;
                        let identify = json!({ "op": 2, "d": {
                            "token": dc.token, "intents": INTENTS,
                            "properties": { "os": std::env::consts::OS, "browser": "alice", "device": "alice" },
                        }});
                        ws.send(Ws::text(identify.to_string())).await.map_err(|e| e.to_string())?;
                    }
                    1 => {
                        ws.send(Ws::text(json!({ "op": 1, "d": seq }).to_string())).await.map_err(|e| e.to_string())?;
                    }
                    7 => return Err("server requested reconnect".into()),
                    9 => return Err("invalid session".into()),
                    0 => {
                        let t = v["t"].as_str().unwrap_or("").to_string();
                        let d = v["d"].clone();
                        let (app, dc) = (app.clone(), dc.clone());
                        tokio::spawn(async move { dispatch(&app, &dc, &t, &d).await });
                    }
                    _ => {}
                }
            }
        }
    }
}
