//! Google account tools (Gmail, Calendar, Drive, Docs, Sheets).
//!
//! OAuth 2.0 for installed apps with PKCE and a loopback redirect, using the
//! user's own OAuth client. Only scopes for enabled services are requested.
//! The refresh token lives in secrets.toml; access tokens only in memory.

use super::{ToolDef, arg_str, untrusted};
use crate::app::App;
use crate::config::{Config, Permission};
use crate::util::{b64_decode, b64_encode, b64url_encode, new_token, urlencode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const SERVICES: [&str; 5] = ["gmail", "calendar", "drive", "docs", "sheets"];

pub fn scopes(services: &[String]) -> Vec<&'static str> {
    let mut s = Vec::new();
    for svc in services {
        match svc.as_str() {
            "gmail" => s.extend(["https://www.googleapis.com/auth/gmail.readonly", "https://www.googleapis.com/auth/gmail.send"]),
            "calendar" => s.push("https://www.googleapis.com/auth/calendar.events"),
            "drive" => s.push("https://www.googleapis.com/auth/drive.readonly"),
            "docs" => s.push("https://www.googleapis.com/auth/documents"),
            "sheets" => s.push("https://www.googleapis.com/auth/spreadsheets"),
            _ => {}
        }
    }
    s
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

pub static DEFS: [ToolDef; 13] = [
    ToolDef { name: "gmail_search", group: "google", default: Permission::Allow,
        description: "Search Gmail with Gmail query syntax (e.g. 'from:alice newer_than:7d'). Returns id, from, subject, date, snippet.",
        params: || obj(json!({"query":{"type":"string"},"max":{"type":"integer","description":"1-25, default 10"}}), &["query"]) },
    ToolDef { name: "gmail_read", group: "google", default: Permission::Allow,
        description: "Read one Gmail message as text.", params: || obj(json!({"id":{"type":"string"}}), &["id"]) },
    ToolDef { name: "gmail_send", group: "google", default: Permission::Ask,
        description: "Send a plain-text email from the user's Gmail account.",
        params: || obj(json!({"to":{"type":"string"},"subject":{"type":"string"},"body":{"type":"string"},"cc":{"type":"string"}}), &["to","subject","body"]) },
    ToolDef { name: "calendar_list", group: "google", default: Permission::Allow,
        description: "List events on the primary calendar. Times are RFC3339 (default: now to 7 days ahead).",
        params: || obj(json!({"time_min":{"type":"string"},"time_max":{"type":"string"},"query":{"type":"string"}}), &[]) },
    ToolDef { name: "calendar_create", group: "google", default: Permission::Ask,
        description: "Create an event on the primary calendar. start/end: RFC3339 date-time with offset, or YYYY-MM-DD for all-day.",
        params: || obj(json!({"summary":{"type":"string"},"start":{"type":"string"},"end":{"type":"string"},"description":{"type":"string"},"location":{"type":"string"},"time_zone":{"type":"string","description":"IANA name, e.g. Asia/Tokyo"}}), &["summary","start","end"]) },
    ToolDef { name: "calendar_delete", group: "google", default: Permission::Ask,
        description: "Delete an event from the primary calendar.", params: || obj(json!({"event_id":{"type":"string"}}), &["event_id"]) },
    ToolDef { name: "drive_search", group: "google", default: Permission::Allow,
        description: "Search Google Drive files by name or content.", params: || obj(json!({"query":{"type":"string"}}), &["query"]) },
    ToolDef { name: "drive_read", group: "google", default: Permission::Allow,
        description: "Read a Drive file as text (Docs → text, Sheets → CSV, text files as-is).", params: || obj(json!({"file_id":{"type":"string"}}), &["file_id"]) },
    ToolDef { name: "docs_create", group: "google", default: Permission::Ask,
        description: "Create a Google Doc with optional initial text.", params: || obj(json!({"title":{"type":"string"},"content":{"type":"string"}}), &["title"]) },
    ToolDef { name: "docs_append", group: "google", default: Permission::Ask,
        description: "Append text to the end of a Google Doc.", params: || obj(json!({"document_id":{"type":"string"},"text":{"type":"string"}}), &["document_id","text"]) },
    ToolDef { name: "sheets_read", group: "google", default: Permission::Allow,
        description: "Read a range from a Google Sheet (A1 notation, e.g. 'Sheet1!A1:D20').",
        params: || obj(json!({"spreadsheet_id":{"type":"string"},"range":{"type":"string"}}), &["spreadsheet_id","range"]) },
    ToolDef { name: "sheets_update", group: "google", default: Permission::Ask,
        description: "Write values into a range of a Google Sheet.",
        params: || obj(json!({"spreadsheet_id":{"type":"string"},"range":{"type":"string"},"values":{"type":"array","items":{"type":"array","items":{}}}}), &["spreadsheet_id","range","values"]) },
    ToolDef { name: "sheets_append", group: "google", default: Permission::Ask,
        description: "Append rows after the last row of a table in a Google Sheet.",
        params: || obj(json!({"spreadsheet_id":{"type":"string"},"range":{"type":"string"},"values":{"type":"array","items":{"type":"array","items":{}}}}), &["spreadsheet_id","range","values"]) },
];

fn service_of(tool: &str) -> &'static str {
    match tool.split('_').next().unwrap_or("") {
        "gmail" => "gmail",
        "calendar" => "calendar",
        "drive" => "drive",
        "docs" => "docs",
        _ => "sheets",
    }
}

pub fn connected(app: &App) -> bool {
    app.secrets().get("google.refresh_token").is_some()
}

pub fn available(cfg: &Config, app: &App, name: &str) -> bool {
    connected(app) && cfg.tools.google.services.iter().any(|s| s == service_of(name))
}

// ------------------------------------------------------------------ endpoints

fn url(cfg: &Config, kind: &str, path: &str) -> String {
    if !cfg.tools.google.api_base.is_empty() {
        return format!("{}/{kind}{path}", cfg.tools.google.api_base.trim_end_matches('/'));
    }
    let base = match kind {
        "auth" => "https://accounts.google.com/o/oauth2/v2/auth",
        "oauth" => "https://oauth2.googleapis.com",
        "gmail" => "https://gmail.googleapis.com/gmail/v1",
        "calendar" => "https://www.googleapis.com/calendar/v3",
        "drive" => "https://www.googleapis.com/drive/v3",
        "docs" => "https://docs.googleapis.com/v1",
        _ => "https://sheets.googleapis.com/v4",
    };
    format!("{base}{path}")
}

// ------------------------------------------------------------------ OAuth

struct Pending {
    state: String,
    verifier: String,
    redirect_uri: String,
    created: Instant,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static ACCESS: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Begin login; returns the URL the user opens in a browser.
pub fn login_url(cfg: &Config, redirect_uri: &str) -> Result<String, String> {
    if cfg.tools.google.client_id.is_empty() {
        return Err("Google OAuth client ID is not set (tools.google.client_id)".into());
    }
    let services: Vec<String> = if cfg.tools.google.services.is_empty() { SERVICES.iter().map(|s| s.to_string()).collect() } else { cfg.tools.google.services.clone() };
    let verifier = new_token() + &new_token();
    let challenge = b64url_encode(&Sha256::digest(verifier.as_bytes()));
    let state = new_token();
    let scope = scopes(&services).join(" ");
    let u = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        url(cfg, "auth", ""),
        urlencode(&cfg.tools.google.client_id),
        urlencode(redirect_uri),
        urlencode(&scope),
        challenge,
        state
    );
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(Pending { state, verifier, redirect_uri: redirect_uri.to_string(), created: Instant::now() });
    Ok(u)
}

/// Complete login with the `code` and `state` from the redirect.
pub async fn finish_login(app: &App, code: &str, state: &str) -> Result<(), String> {
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or("no login in progress")?;
    if !crate::util::ct_eq(pending.state.as_bytes(), state.as_bytes()) {
        return Err("state mismatch; start the login again".into());
    }
    if pending.created.elapsed() > Duration::from_secs(900) {
        return Err("login expired; start again".into());
    }
    let cfg = app.config();
    let secret = app.secrets().get("google.client_secret").map(str::to_string).unwrap_or_default();
    let r = app
        .http
        .post(url(&cfg, "oauth", "/token"))
        .form(&[
            ("client_id", cfg.tools.google.client_id.as_str()),
            ("client_secret", secret.as_str()),
            ("code", code),
            ("code_verifier", pending.verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", pending.redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let v: Value = r.json().await.map_err(|e| e.without_url().to_string())?;
    let refresh = v["refresh_token"].as_str().ok_or_else(|| format!("token exchange failed: {}", v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("no refresh token")))?;
    app.update_secrets(|s| s.set("google.refresh_token", refresh))?;
    if let (Some(at), Some(exp)) = (v["access_token"].as_str(), v["expires_in"].as_u64()) {
        *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = Some((at.to_string(), Instant::now() + Duration::from_secs(exp.saturating_sub(60))));
    }
    Ok(())
}

/// Parse `code` and `state` from a pasted redirect URL.
pub fn parse_redirect(u: &str) -> Option<(String, String)> {
    let q = u.split_once('?')?.1;
    let mut code = None;
    let mut state = None;
    for kv in q.split('&') {
        let (k, v) = kv.split_once('=')?;
        let v = percent_decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            _ => {}
        }
    }
    Some((code?, state?))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(n) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub async fn logout(app: &App) -> Result<(), String> {
    if let Some(t) = app.secrets().get("google.refresh_token").map(str::to_string) {
        let cfg = app.config();
        // Revoke at Google (best effort), then forget locally.
        let _ = app.http.post(url(&cfg, "oauth", "/revoke")).form(&[("token", t.as_str())]).send().await;
    }
    *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    app.update_secrets(|s| s.set("google.refresh_token", ""))
}

async fn access_token(app: &App, cfg: &Config) -> Result<String, String> {
    if let Some((t, exp)) = ACCESS.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if Instant::now() < exp {
            return Ok(t);
        }
    }
    let secrets = app.secrets();
    let refresh = secrets.get("google.refresh_token").ok_or("Google account is not connected")?;
    let r = app
        .http
        .post(url(cfg, "oauth", "/token"))
        .form(&[
            ("client_id", cfg.tools.google.client_id.as_str()),
            ("client_secret", secrets.get("google.client_secret").unwrap_or("")),
            ("refresh_token", refresh),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let v: Value = r.json().await.map_err(|e| e.without_url().to_string())?;
    let Some(at) = v["access_token"].as_str() else {
        let err = v["error"].as_str().unwrap_or("token refresh failed");
        return Err(if err == "invalid_grant" {
            "Google authorization expired or was revoked; reconnect with `alice google login`".into()
        } else {
            format!("Google token refresh failed: {err}")
        });
    };
    let exp = Instant::now() + Duration::from_secs(v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60));
    *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = Some((at.to_string(), exp));
    Ok(at.to_string())
}

async fn call(app: &App, cfg: &Config, method: reqwest::Method, u: String, body: Option<&Value>) -> Result<Value, String> {
    let token = access_token(app, cfg).await?;
    let mut rb = app.http.request(method, u).bearer_auth(token).timeout(Duration::from_secs(30));
    if let Some(b) = body {
        rb = rb.json(b);
    }
    let r = rb.send().await.map_err(|e| e.without_url().to_string())?;
    let status = r.status();
    let text = r.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        return Err(format!("Google API HTTP {}: {}", status.as_u16(), v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("")));
    }
    Ok(if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) })
}

async fn call_text(app: &App, cfg: &Config, u: String) -> Result<String, String> {
    let token = access_token(app, cfg).await?;
    let r = app.http.get(u).bearer_auth(token).timeout(Duration::from_secs(30)).send().await.map_err(|e| e.without_url().to_string())?;
    if !r.status().is_success() {
        return Err(format!("Google API HTTP {}", r.status().as_u16()));
    }
    let b = r.bytes().await.map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&b[..b.len().min(400_000)]).into_owned())
}

// ------------------------------------------------------------------ tools

fn no_newlines(s: &str, what: &str) -> Result<(), String> {
    if s.contains(['\r', '\n']) { Err(format!("{what} must not contain line breaks")) } else { Ok(()) }
}

fn mime_header(s: &str) -> String {
    if s.is_ascii() { s.to_string() } else { format!("=?UTF-8?B?{}?=", b64_encode(s.as_bytes())) }
}

pub fn build_email(to: &str, cc: Option<&str>, subject: &str, body: &str) -> Result<String, String> {
    no_newlines(to, "to")?;
    no_newlines(subject, "subject")?;
    let mut m = format!("To: {to}\r\n");
    if let Some(cc) = cc.filter(|c| !c.is_empty()) {
        no_newlines(cc, "cc")?;
        m.push_str(&format!("Cc: {cc}\r\n"));
    }
    m.push_str(&format!(
        "Subject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
        mime_header(subject),
        b64_encode(body.as_bytes())
    ));
    Ok(b64url_encode(m.as_bytes()))
}

fn gmail_text(payload: &Value) -> String {
    // Prefer text/plain; fall back to stripped text/html.
    fn walk(p: &Value, want: &str, out: &mut Option<String>) {
        if out.is_some() {
            return;
        }
        if p["mimeType"].as_str() == Some(want) {
            if let Some(d) = p.pointer("/body/data").and_then(|d| d.as_str()).and_then(b64_decode) {
                *out = Some(String::from_utf8_lossy(&d).into_owned());
                return;
            }
        }
        for part in p["parts"].as_array().into_iter().flatten() {
            walk(part, want, out);
        }
    }
    let mut t = None;
    walk(payload, "text/plain", &mut t);
    if t.is_none() {
        let mut h = None;
        walk(payload, "text/html", &mut h);
        t = h.map(|h| super::web::extract(&h).1);
    }
    t.unwrap_or_default()
}

fn header(headers: &Value, name: &str) -> String {
    headers
        .as_array()
        .and_then(|a| a.iter().find(|h| h["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))))
        .and_then(|h| h["value"].as_str())
        .unwrap_or("")
        .to_string()
}

fn event_time(s: &str, tz: Option<&str>) -> Value {
    if s.contains('T') {
        let mut v = json!({ "dateTime": s });
        if let Some(tz) = tz {
            v["timeZone"] = json!(tz);
        }
        v
    } else {
        json!({ "date": s })
    }
}

pub async fn execute(app: &App, cfg: &Config, name: &str, args: &Value) -> Result<String, String> {
    use reqwest::Method as M;
    let s = |k: &str| args.get(k).and_then(|v| v.as_str());
    match name {
        "gmail_search" => {
            let max = args.get("max").and_then(|v| v.as_u64()).unwrap_or(10).clamp(1, 25);
            let list = call(app, cfg, M::GET, url(cfg, "gmail", &format!("/users/me/messages?q={}&maxResults={max}", urlencode(arg_str(args, "query")?))), None).await?;
            let mut out = Vec::new();
            for m in list["messages"].as_array().into_iter().flatten() {
                let id = m["id"].as_str().unwrap_or("");
                let d = call(app, cfg, M::GET, url(cfg, "gmail", &format!("/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date", urlencode(id))), None).await?;
                let h = &d["payload"]["headers"];
                out.push(json!({ "id": id, "from": header(h, "From"), "subject": header(h, "Subject"), "date": header(h, "Date"), "snippet": d["snippet"] }));
            }
            Ok(untrusted("gmail", &serde_json::to_string_pretty(&out).unwrap_or_default()))
        }
        "gmail_read" => {
            let id = arg_str(args, "id")?;
            let d = call(app, cfg, M::GET, url(cfg, "gmail", &format!("/users/me/messages/{}?format=full", urlencode(id))), None).await?;
            let h = &d["payload"]["headers"];
            let body = gmail_text(&d["payload"]);
            let text = format!(
                "From: {}\nTo: {}\nDate: {}\nSubject: {}\n\n{}",
                header(h, "From"), header(h, "To"), header(h, "Date"), header(h, "Subject"),
                crate::util::truncate(&body, 100_000)
            );
            Ok(untrusted(&format!("gmail:{id}"), &text))
        }
        "gmail_send" => {
            let raw = build_email(arg_str(args, "to")?, s("cc"), arg_str(args, "subject")?, arg_str(args, "body")?)?;
            let r = call(app, cfg, M::POST, url(cfg, "gmail", "/users/me/messages/send"), Some(&json!({ "raw": raw }))).await?;
            Ok(format!("sent (id {})", r["id"].as_str().unwrap_or("?")))
        }
        "calendar_list" => {
            let now = crate::util::now_ms();
            let iso = |ms: i64| crate::util::fmt_utc(ms).replace(' ', "T") + "Z";
            let min = s("time_min").map(str::to_string).unwrap_or_else(|| iso(now));
            let max = s("time_max").map(str::to_string).unwrap_or_else(|| iso(now + 7 * 86_400_000));
            let mut q = format!("/calendars/primary/events?singleEvents=true&orderBy=startTime&maxResults=50&timeMin={}&timeMax={}", urlencode(&min), urlencode(&max));
            if let Some(text) = s("query") {
                q.push_str(&format!("&q={}", urlencode(text)));
            }
            let v = call(app, cfg, M::GET, url(cfg, "calendar", &q), None).await?;
            let items: Vec<Value> = v["items"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| json!({ "id": e["id"], "summary": e["summary"], "start": e["start"], "end": e["end"], "location": e["location"], "description": e["description"] }))
                .collect();
            Ok(untrusted("calendar", &serde_json::to_string_pretty(&items).unwrap_or_default()))
        }
        "calendar_create" => {
            let tz = s("time_zone");
            let mut ev = json!({ "summary": arg_str(args, "summary")?, "start": event_time(arg_str(args, "start")?, tz), "end": event_time(arg_str(args, "end")?, tz) });
            if let Some(d) = s("description") {
                ev["description"] = json!(d);
            }
            if let Some(l) = s("location") {
                ev["location"] = json!(l);
            }
            let r = call(app, cfg, M::POST, url(cfg, "calendar", "/calendars/primary/events"), Some(&ev)).await?;
            Ok(format!("created event {} ({})", r["id"].as_str().unwrap_or("?"), r["htmlLink"].as_str().unwrap_or("")))
        }
        "calendar_delete" => {
            let id = arg_str(args, "event_id")?;
            call(app, cfg, M::DELETE, url(cfg, "calendar", &format!("/calendars/primary/events/{}", urlencode(id))), None).await?;
            Ok(format!("deleted event {id}"))
        }
        "drive_search" => {
            let qs = arg_str(args, "query")?.replace('\\', "\\\\").replace('\'', "\\'");
            let q = format!("(name contains '{qs}' or fullText contains '{qs}') and trashed = false");
            let v = call(app, cfg, M::GET, url(cfg, "drive", &format!("/files?pageSize=20&fields=files(id,name,mimeType,modifiedTime,webViewLink)&q={}", urlencode(&q))), None).await?;
            Ok(untrusted("drive", &serde_json::to_string_pretty(&v["files"]).unwrap_or_default()))
        }
        "drive_read" => {
            let id = urlencode(arg_str(args, "file_id")?);
            let meta = call(app, cfg, M::GET, url(cfg, "drive", &format!("/files/{id}?fields=id,name,mimeType,size")), None).await?;
            let mime = meta["mimeType"].as_str().unwrap_or("");
            let body = match mime {
                "application/vnd.google-apps.document" | "application/vnd.google-apps.presentation" => {
                    call_text(app, cfg, url(cfg, "drive", &format!("/files/{id}/export?mimeType=text%2Fplain"))).await?
                }
                "application/vnd.google-apps.spreadsheet" => call_text(app, cfg, url(cfg, "drive", &format!("/files/{id}/export?mimeType=text%2Fcsv"))).await?,
                m if m.starts_with("text/") || m.contains("json") || m.contains("xml") => call_text(app, cfg, url(cfg, "drive", &format!("/files/{id}?alt=media"))).await?,
                m => return Err(format!("cannot read {m} files as text")),
            };
            Ok(untrusted(&format!("drive:{}", meta["name"].as_str().unwrap_or("")), crate::util::truncate(&body, 150_000)))
        }
        "docs_create" => {
            let d = call(app, cfg, M::POST, url(cfg, "docs", "/documents"), Some(&json!({ "title": arg_str(args, "title")? }))).await?;
            let id = d["documentId"].as_str().unwrap_or("").to_string();
            if let Some(c) = s("content").filter(|c| !c.is_empty()) {
                let req = json!({ "requests": [{ "insertText": { "location": { "index": 1 }, "text": c } }] });
                call(app, cfg, M::POST, url(cfg, "docs", &format!("/documents/{}:batchUpdate", urlencode(&id))), Some(&req)).await?;
            }
            Ok(format!("created document {id} (https://docs.google.com/document/d/{id}/edit)"))
        }
        "docs_append" => {
            let id = arg_str(args, "document_id")?;
            let req = json!({ "requests": [{ "insertText": { "endOfSegmentLocation": {}, "text": arg_str(args, "text")? } }] });
            call(app, cfg, M::POST, url(cfg, "docs", &format!("/documents/{}:batchUpdate", urlencode(id))), Some(&req)).await?;
            Ok(format!("appended to {id}"))
        }
        "sheets_read" => {
            let (id, range) = (urlencode(arg_str(args, "spreadsheet_id")?), urlencode(arg_str(args, "range")?));
            let v = call(app, cfg, M::GET, url(cfg, "sheets", &format!("/spreadsheets/{id}/values/{range}")), None).await?;
            Ok(untrusted("sheets", &serde_json::to_string(&v["values"]).unwrap_or_default()))
        }
        "sheets_update" | "sheets_append" => {
            let (id, range) = (urlencode(arg_str(args, "spreadsheet_id")?), urlencode(arg_str(args, "range")?));
            let values = args.get("values").filter(|v| v.is_array()).ok_or("values must be an array of rows")?;
            let body = json!({ "values": values });
            if name == "sheets_update" {
                let r = call(app, cfg, M::PUT, url(cfg, "sheets", &format!("/spreadsheets/{id}/values/{range}?valueInputOption=USER_ENTERED")), Some(&body)).await?;
                Ok(format!("updated {} cells", r["updatedCells"].as_u64().unwrap_or(0)))
            } else {
                let r = call(app, cfg, M::POST, url(cfg, "sheets", &format!("/spreadsheets/{id}/values/{range}:append?valueInputOption=USER_ENTERED&insertDataOption=INSERT_ROWS")), Some(&body)).await?;
                Ok(format!("appended {} rows", r.pointer("/updates/updatedRows").and_then(|n| n.as_u64()).unwrap_or(0)))
            }
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_encoded_and_injection_safe() {
        let raw = build_email("a@example.com", None, "こんにちは", "本文").unwrap();
        let m = String::from_utf8(b64_decode(&raw).unwrap()).unwrap();
        assert!(m.starts_with("To: a@example.com\r\n"));
        assert!(m.contains("Subject: =?UTF-8?B?"));
        assert!(build_email("a@example.com\r\nBcc: evil@example.com", None, "s", "b").is_err());
        assert!(build_email("a@example.com", None, "s\nBcc: x", "b").is_err());
    }

    #[test]
    fn scopes_follow_services() {
        assert_eq!(scopes(&["calendar".into()]), vec!["https://www.googleapis.com/auth/calendar.events"]);
        assert!(scopes(&["gmail".into()]).iter().all(|s| s.contains("gmail")));
    }

    #[test]
    fn redirect_parsing() {
        assert_eq!(
            parse_redirect("http://127.0.0.1:7878/oauth/google/callback?state=abc&code=4%2F0Ax&scope=x").unwrap(),
            ("4/0Ax".to_string(), "abc".to_string())
        );
    }
}
