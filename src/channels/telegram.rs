//! Telegram Bot API gateway (long polling; no public endpoint needed).

use super::{Button, Incoming, Surface};
use crate::app::AppRef;
use crate::{info, warn};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const MAX_FILE: u64 = 20 * 1024 * 1024;

pub struct Telegram {
    http: reqwest::Client,
    base: String,
    file_base: String,
}

impl Telegram {
    pub fn new(app: &AppRef, token: &str) -> Telegram {
        let api = app.config().channels.telegram.api_base;
        let root = if api.is_empty() { "https://api.telegram.org".to_string() } else { api.trim_end_matches('/').to_string() };
        Telegram {
            http: reqwest::Client::builder().timeout(Duration::from_secs(70)).build().expect("http"),
            base: format!("{root}/bot{token}"),
            file_base: format!("{root}/file/bot{token}"),
        }
    }

    /// Call a Bot API method. Errors never include the URL (it contains the token).
    pub async fn call(&self, method: &str, body: &Value) -> Result<Value, String> {
        let r = self
            .http
            .post(format!("{}/{method}", self.base))
            .json(body)
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let v: Value = r.json().await.map_err(|e| e.without_url().to_string())?;
        if v["ok"] == true {
            Ok(v["result"].clone())
        } else {
            if let Some(after) = v.pointer("/parameters/retry_after").and_then(|n| n.as_u64()) {
                tokio::time::sleep(Duration::from_secs(after.min(30))).await;
            }
            Err(v["description"].as_str().unwrap_or("telegram error").to_string())
        }
    }

    async fn download(&self, file_id: &str) -> Result<Vec<u8>, String> {
        let f = self.call("getFile", &json!({ "file_id": file_id })).await?;
        if f["file_size"].as_u64().unwrap_or(0) > MAX_FILE {
            return Err("file too large".into());
        }
        let path = f["file_path"].as_str().ok_or("no file path")?;
        let r = self
            .http
            .get(format!("{}/{path}", self.file_base))
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let b = r.bytes().await.map_err(|e| e.without_url().to_string())?;
        Ok(b.to_vec())
    }

    async fn send_or_edit(&self, chat: &str, id: Option<&str>, text: &str, buttons: &[Button]) -> Result<Value, String> {
        let mut body = json!({ "chat_id": chat, "text": md_to_html(text), "parse_mode": "HTML", "link_preview_options": { "is_disabled": true } });
        if !buttons.is_empty() {
            body["reply_markup"] = keyboard(buttons);
        }
        let method = if let Some(id) = id {
            body["message_id"] = json!(id.parse::<i64>().unwrap_or(0));
            "editMessageText"
        } else {
            "sendMessage"
        };
        match self.call(method, &body).await {
            Ok(v) => Ok(v),
            Err(e) if e.contains("message is not modified") => Ok(Value::Null),
            Err(e) if e.contains("parse entities") => {
                // Fall back to plain text.
                body["text"] = json!(text);
                body.as_object_mut().unwrap().remove("parse_mode");
                self.call(method, &body).await
            }
            Err(e) => Err(e),
        }
    }
}

fn keyboard(buttons: &[Button]) -> Value {
    let rows: Vec<Value> = buttons
        .chunks(if buttons.len() > 4 { 1 } else { 2 })
        .map(|row| Value::Array(row.iter().map(|b| json!({ "text": b.label, "callback_data": b.data })).collect()))
        .collect();
    json!({ "inline_keyboard": rows })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Minimal Markdown → Telegram HTML (code blocks, inline code, bold, links).
pub fn md_to_html(src: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let mut code = String::new();
    for line in src.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            if in_code {
                out.push_str(&format!("<pre>{}</pre>\n", esc(code.trim_end_matches('\n'))));
                code.clear();
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push_str(line);
            continue;
        }
        out.push_str(&inline(line));
    }
    if in_code {
        out.push_str(&format!("<pre>{}</pre>", esc(code.trim_end_matches('\n'))));
    }
    out
}

fn inline(line: &str) -> String {
    let mut out = String::new();
    for (i, part) in line.split('`').enumerate() {
        if i % 2 == 1 {
            out.push_str(&format!("<code>{}</code>", esc(part)));
            continue;
        }
        let mut t = esc(part);
        // Headings → bold.
        let trimmed = t.trim_start_matches('#');
        if trimmed.len() < t.len() && trimmed.starts_with(' ') {
            t = format!("<b>{}</b>", trimmed.trim_start().trim_end_matches('\n')) + if part.ends_with('\n') { "\n" } else { "" };
        }
        // **bold**
        let mut b = String::new();
        let mut rest = t.as_str();
        while let Some(i) = rest.find("**") {
            match rest[i + 2..].find("**") {
                Some(j) => {
                    b.push_str(&rest[..i]);
                    b.push_str(&format!("<b>{}</b>", &rest[i + 2..i + 2 + j]));
                    rest = &rest[i + 4 + j..];
                }
                None => break,
            }
        }
        b.push_str(rest);
        // [text](https://url)
        let mut l = String::new();
        let mut rest = b.as_str();
        while let Some(i) = rest.find('[') {
            let Some(j) = rest[i..].find("](") else { break };
            let Some(k) = rest[i + j..].find(')') else { break };
            let text = &rest[i + 1..i + j];
            let url = &rest[i + j + 2..i + j + k];
            if url.starts_with("https://") || url.starts_with("http://") {
                l.push_str(&rest[..i]);
                l.push_str(&format!("<a href=\"{}\">{}</a>", url.replace('"', "&quot;"), text));
            } else {
                l.push_str(&rest[..i + j + k + 1]);
            }
            rest = &rest[i + j + k + 1..];
        }
        l.push_str(rest);
        out.push_str(&l);
    }
    out
}

impl Surface for Telegram {
    fn channel(&self) -> &'static str {
        "telegram"
    }
    fn max_len(&self) -> usize {
        3800 // 4096 limit, leaving room for HTML tags
    }
    async fn send(&self, chat: &str, text: &str, buttons: &[Button]) -> Result<String, String> {
        let v = self.send_or_edit(chat, None, text, buttons).await?;
        Ok(v["message_id"].as_i64().map(|i| i.to_string()).unwrap_or_default())
    }
    async fn edit(&self, chat: &str, id: &str, text: &str, buttons: &[Button]) -> Result<(), String> {
        self.send_or_edit(chat, Some(id), text, buttons).await.map(|_| ())
    }
    async fn delete(&self, chat: &str, id: &str) -> Result<(), String> {
        self.call("deleteMessage", &json!({ "chat_id": chat, "message_id": id.parse::<i64>().unwrap_or(0) })).await.map(|_| ())
    }
    async fn typing(&self, chat: &str) {
        let _ = self.call("sendChatAction", &json!({ "chat_id": chat, "action": "typing" })).await;
    }
}

fn allowed(app: &AppRef, user: Option<i64>) -> bool {
    user.is_some_and(|u| app.config().channels.telegram.allowed_user_ids.contains(&u))
}

async fn incoming(app: &AppRef, tg: &Arc<Telegram>, m: &Value) -> Option<Incoming> {
    let chat_id = m.pointer("/chat/id")?.as_i64()?.to_string();
    let mut files = Vec::new();
    if let Some(photos) = m["photo"].as_array() {
        if let Some(best) = photos.last() {
            if let Ok(b) = tg.download(best["file_id"].as_str().unwrap_or("")).await {
                files.push(("photo.jpg".into(), "image/jpeg".into(), b));
            }
        }
    }
    if let Some(doc) = m.get("document") {
        match tg.download(doc["file_id"].as_str().unwrap_or("")).await {
            Ok(b) => files.push((
                doc["file_name"].as_str().unwrap_or("file").to_string(),
                doc["mime_type"].as_str().unwrap_or("").to_string(),
                b,
            )),
            Err(e) => {
                let _ = tg.send(&chat_id, &format!("Could not read the file: {e}"), &[]).await;
            }
        }
    }
    let text = m["text"].as_str().or(m["caption"].as_str()).unwrap_or("").to_string();
    if text.is_empty() && files.is_empty() {
        return None;
    }
    let _ = app;
    Some(Incoming { chat_id, message_id: m["message_id"].as_i64().unwrap_or(0).to_string(), text, files })
}

pub async fn run(app: AppRef, token: String, stop: CancellationToken) {
    let tg = Arc::new(Telegram::new(&app, &token));
    match tg.call("getMe", &json!({})).await {
        Ok(me) => info!("telegram: connected as @{}", me["username"].as_str().unwrap_or("?")),
        Err(e) => {
            warn!("telegram: cannot connect: {e}");
            return;
        }
    }
    let commands: Vec<Value> = [
        ("new", "Start a new thread"),
        ("threads", "List and switch threads"),
        ("model", "Show or change the model"),
        ("delete", "Delete the current thread"),
        ("stop", "Stop the current response"),
        ("status", "Show status"),
        ("help", "Help"),
    ]
    .iter()
    .map(|(c, d)| json!({ "command": c, "description": d }))
    .collect();
    let _ = tg.call("setMyCommands", &json!({ "commands": commands })).await;

    // Deletion of posted messages when threads are deleted elsewhere.
    {
        let tg = tg.clone();
        let mut rx = app.events.subscribe();
        let stop = stop.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.cancelled() => return,
                    ev = rx.recv() => match ev {
                        Ok(ev) if ev.json.contains("\"thread.deleted\"") || ev.json.contains("\"threads.cleared\"") => {
                            if let Ok(v) = serde_json::from_str::<Value>(&ev.json) {
                                super::delete_external(&tg, &v).await;
                            }
                        }
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => return,
                    }
                }
            }
        });
    }

    let mut offset: i64 = 0;
    let mut backoff = 1u64;
    loop {
        let body = json!({ "offset": offset, "timeout": 50, "allowed_updates": ["message", "callback_query"] });
        let r = tokio::select! {
            _ = stop.cancelled() => return,
            r = tg.call("getUpdates", &body) => r,
        };
        let updates = match r {
            Ok(v) => {
                backoff = 1;
                v.as_array().cloned().unwrap_or_default()
            }
            Err(e) => {
                warn!("telegram: getUpdates failed: {}", crate::util::truncate(&e, 200));
                tokio::select! {
                    _ = stop.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                }
                backoff = (backoff * 2).min(60);
                continue;
            }
        };
        for u in updates {
            offset = offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
            if let Some(m) = u.get("message") {
                let user = m.pointer("/from/id").and_then(|i| i.as_i64());
                if !allowed(&app, user) {
                    warn!("telegram: ignored message from user {:?} (not in allowlist)", user);
                    continue;
                }
                let (app, tg, m) = (app.clone(), tg.clone(), m.clone());
                tokio::spawn(async move {
                    if let Some(msg) = incoming(&app, &tg, &m).await {
                        super::handle_message(&app, &tg, msg).await;
                    }
                });
            } else if let Some(q) = u.get("callback_query") {
                let user = q.pointer("/from/id").and_then(|i| i.as_i64());
                if !allowed(&app, user) {
                    continue;
                }
                let (app, tg, q) = (app.clone(), tg.clone(), q.clone());
                tokio::spawn(async move {
                    let chat = q.pointer("/message/chat/id").and_then(|c| c.as_i64()).map(|c| c.to_string()).unwrap_or_default();
                    let data = q["data"].as_str().unwrap_or("");
                    let status = super::handle_button(&app, &tg, &chat, data).await;
                    let _ = tg.call("answerCallbackQuery", &json!({ "callback_query_id": q["id"], "text": status })).await;
                    // Replace the buttons with the outcome.
                    if let Some(mid) = q.pointer("/message/message_id").and_then(|m| m.as_i64()) {
                        let old = q.pointer("/message/text").and_then(|t| t.as_str()).unwrap_or("");
                        let _ = tg
                            .call("editMessageText", &json!({ "chat_id": chat, "message_id": mid, "text": format!("{old}\n→ {status}") }))
                            .await;
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_conversion_is_escaped() {
        let h = md_to_html("**hi** <b> & `x<y`\n```\nlet a = 1 < 2;\n```\n[l](https://e.com) [j](javascript:x)");
        assert!(h.contains("<b>hi</b>"));
        assert!(h.contains("&lt;b&gt; &amp;"));
        assert!(h.contains("<code>x&lt;y</code>"));
        assert!(h.contains("<pre>let a = 1 &lt; 2;</pre>"));
        assert!(h.contains("<a href=\"https://e.com\">l</a>"));
        assert!(!h.contains("href=\"javascript"));
    }
}
