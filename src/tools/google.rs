//! Built-in handler for the google-workspace skill (Gmail, Calendar, Tasks,
//! Drive, Docs, Sheets). Tool definitions, scopes and OAuth live in the skill
//! (`skills/google-workspace`); this module only talks to the Google APIs.
//! The data functions are shared by the tools and the app API
//! (`/api/google/*`, used by the iOS app).

use super::{arg_str, untrusted};
use crate::app::App;
use crate::skills::Skill;
use crate::util::{b64_decode, b64_encode, b64url_encode, urlencode};
use serde_json::{Value, json};
use std::time::Duration;

/// Request context: the skill (for its token) and an optional API base override.
pub struct G<'a> {
    app: &'a App,
    skill: &'a Skill,
    base: Option<String>,
}

impl<'a> G<'a> {
    pub fn new(app: &'a App, skill: &'a Skill) -> Self {
        let cfg = app.config();
        G { app, skill, base: crate::skills::setting(app, &cfg, skill, "api_base") }
    }
}

// ------------------------------------------------------------------ endpoints

fn url(g: &G, kind: &str, path: &str) -> String {
    if let Some(b) = &g.base {
        return format!("{}/{kind}{path}", b.trim_end_matches('/'));
    }
    let base = match kind {
        "gmail" => "https://gmail.googleapis.com/gmail/v1",
        "calendar" => "https://www.googleapis.com/calendar/v3",
        "drive" => "https://www.googleapis.com/drive/v3",
        "docs" => "https://docs.googleapis.com/v1",
        "tasks" => "https://tasks.googleapis.com/tasks/v1",
        _ => "https://sheets.googleapis.com/v4",
    };
    format!("{base}{path}")
}

async fn call(g: &G<'_>, method: reqwest::Method, u: String, body: Option<&Value>) -> Result<Value, String> {
    let token = crate::skills::auth::access_token(g.app, g.skill).await?;
    let build = || {
        let mut rb = g.app.http.request(method.clone(), &u).bearer_auth(&token).timeout(Duration::from_secs(30));
        if let Some(b) = body {
            rb = rb.json(b);
        }
        rb
    };
    // A dropped keep-alive connection fails before anything is sent; reads retry once.
    let r = match build().send().await {
        Err(e) if method == reqwest::Method::GET && !e.is_timeout() => build().send().await,
        r => r,
    }
    .map_err(|e| e.without_url().to_string())?;
    let status = r.status();
    let text = r.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        return Err(format!("Google API HTTP {}: {}", status.as_u16(), v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("")));
    }
    Ok(if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) })
}

async fn call_text(g: &G<'_>, u: String) -> Result<String, String> {
    let token = crate::skills::auth::access_token(g.app, g.skill).await?;
    let r = g.app.http.get(u).bearer_auth(token).timeout(Duration::from_secs(30)).send().await.map_err(|e| e.without_url().to_string())?;
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

/// `reply`: (In-Reply-To, References) of the message being answered.
pub fn build_email(to: &str, cc: Option<&str>, subject: &str, body: &str, reply: Option<(&str, &str)>) -> Result<String, String> {
    no_newlines(to, "to")?;
    no_newlines(subject, "subject")?;
    let mut m = format!("To: {to}\r\n");
    if let Some(cc) = cc.filter(|c| !c.is_empty()) {
        no_newlines(cc, "cc")?;
        m.push_str(&format!("Cc: {cc}\r\n"));
    }
    if let Some((irt, refs)) = reply.filter(|(irt, _)| !irt.is_empty()) {
        no_newlines(irt, "in-reply-to")?;
        no_newlines(refs, "references")?;
        m.push_str(&format!("In-Reply-To: {irt}\r\nReferences: {}\r\n", format!("{refs} {irt}").trim()));
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
        if p["mimeType"].as_str() == Some(want)
            && let Some(d) = p.pointer("/body/data").and_then(|d| d.as_str()).and_then(b64_decode) {
                *out = Some(String::from_utf8_lossy(&d).into_owned());
                return;
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

// ------------------------------------------------------------------ data

use reqwest::Method as M;

fn iso(ms: i64) -> String {
    crate::util::fmt_utc(ms).replace(' ', "T") + "Z"
}

fn event_json(e: &Value) -> Value {
    let when = |k: &str| e[k]["dateTime"].as_str().or(e[k]["date"].as_str()).unwrap_or("").to_string();
    json!({
        "id": e["id"], "summary": e["summary"].as_str().unwrap_or(""), "start": when("start"), "end": when("end"),
        "all_day": e["start"]["date"].is_string(), "location": e["location"], "description": e["description"],
        "color_id": e["colorId"], "link": e["htmlLink"],
    })
}

/// Events on the primary calendar between two RFC3339 instants.
pub async fn calendar_events(g: &G<'_>, min: &str, max: &str, query: Option<&str>, limit: u32) -> Result<Vec<Value>, String> {
    let mut q = format!("/calendars/primary/events?singleEvents=true&orderBy=startTime&maxResults={limit}&timeMin={}&timeMax={}", urlencode(min), urlencode(max));
    if let Some(text) = query.filter(|t| !t.is_empty()) {
        q.push_str(&format!("&q={}", urlencode(text)));
    }
    let v = call(g, M::GET, url(g, "calendar", &q), None).await?;
    Ok(v["items"].as_array().into_iter().flatten().map(event_json).collect())
}

/// Create an event. `start`/`end`: RFC3339 date-time, or YYYY-MM-DD for all-day.
pub async fn calendar_create(g: &G<'_>, args: &Value) -> Result<Value, String> {
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty());
    let tz = s("time_zone");
    let mut ev = json!({ "summary": arg_str(args, "summary")?, "start": event_time(arg_str(args, "start")?, tz), "end": event_time(arg_str(args, "end")?, tz) });
    for (k, f) in [("description", "description"), ("location", "location")] {
        if let Some(v) = s(k) {
            ev[f] = json!(v);
        }
    }
    Ok(event_json(&call(g, M::POST, url(g, "calendar", "/calendars/primary/events"), Some(&ev)).await?))
}

pub async fn calendar_delete(g: &G<'_>, id: &str) -> Result<(), String> {
    call(g, M::DELETE, url(g, "calendar", &format!("/calendars/primary/events/{}", urlencode(id))), None).await.map(|_| ())
}

/// Decode the few HTML entities Gmail puts in snippets.
fn unescape(s: &str) -> String {
    s.replace("&#39;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn mail_summary(d: &Value) -> Value {
    let h = &d["payload"]["headers"];
    let labels = d["labelIds"].as_array().cloned().unwrap_or_default();
    json!({
        "id": d["id"], "thread_id": d["threadId"], "from": header(h, "From"), "to": header(h, "To"), "subject": header(h, "Subject"),
        "date": header(h, "Date"), "date_ms": d["internalDate"].as_str().and_then(|n| n.parse::<i64>().ok()),
        "snippet": unescape(d["snippet"].as_str().unwrap_or("")), "unread": labels.iter().any(|l| l == "UNREAD"),
    })
}

/// Messages matching a Gmail query, newest first, with headers and snippet.
pub async fn gmail_list(g: &G<'_>, query: &str, max: u32, page: Option<&str>) -> Result<(Vec<Value>, Option<String>), String> {
    let mut q = format!("/users/me/messages?maxResults={max}&q={}", urlencode(query));
    if let Some(p) = page.filter(|p| !p.is_empty()) {
        q.push_str(&format!("&pageToken={}", urlencode(p)));
    }
    let list = call(g, M::GET, url(g, "gmail", &q), None).await?;
    let ids: Vec<String> = list["messages"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(str::to_string)).collect();
    // At most 8 requests at a time.
    let mut out = Vec::new();
    for chunk in ids.chunks(8) {
        let fetch = chunk.iter().map(|id| {
            call(g, M::GET, url(g, "gmail", &format!("/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=To&metadataHeaders=Subject&metadataHeaders=Date", urlencode(id))), None)
        });
        for d in futures_util::future::join_all(fetch).await {
            out.push(mail_summary(&d?));
        }
    }
    Ok((out, list["nextPageToken"].as_str().map(str::to_string)))
}

/// One message with its text body.
pub async fn gmail_get(g: &G<'_>, id: &str) -> Result<Value, String> {
    let d = call(g, M::GET, url(g, "gmail", &format!("/users/me/messages/{}?format=full", urlencode(id))), None).await?;
    let mut v = mail_summary(&d);
    let h = &d["payload"]["headers"];
    v["cc"] = json!(header(h, "Cc"));
    v["reply_to"] = json!(header(h, "Reply-To"));
    v["body"] = json!(crate::util::truncate(&gmail_text(&d["payload"]), 200_000));
    Ok(v)
}

/// Send a plain-text email; `reply_to_id` threads it as a reply to that message.
pub async fn gmail_send(g: &G<'_>, to: &str, cc: Option<&str>, subject: &str, body: &str, reply_to_id: Option<&str>) -> Result<String, String> {
    let mut thread = None;
    let mut reply = None;
    if let Some(id) = reply_to_id.filter(|i| !i.is_empty()) {
        let d = call(g, M::GET, url(g, "gmail", &format!("/users/me/messages/{}?format=metadata&metadataHeaders=Message-ID&metadataHeaders=References", urlencode(id))), None).await?;
        let h = &d["payload"]["headers"];
        reply = Some((header(h, "Message-ID"), header(h, "References")));
        thread = d["threadId"].as_str().map(str::to_string);
    }
    let raw = build_email(to, cc, subject, body, reply.as_ref().map(|(a, b)| (a.as_str(), b.as_str())))?;
    let mut req = json!({ "raw": raw });
    if let Some(t) = thread {
        req["threadId"] = json!(t);
    }
    let r = call(g, M::POST, url(g, "gmail", "/users/me/messages/send"), Some(&req)).await?;
    Ok(r["id"].as_str().unwrap_or("").to_string())
}

/// Mark a message read (or unread again). Needs the gmail.modify scope.
pub async fn gmail_mark_read(g: &G<'_>, id: &str, read: bool) -> Result<(), String> {
    let labels = json!(["UNREAD"]);
    let body = if read { json!({ "removeLabelIds": labels }) } else { json!({ "addLabelIds": labels }) };
    call(g, M::POST, url(g, "gmail", &format!("/users/me/messages/{}/modify", urlencode(id))), Some(&body)).await?;
    Ok(())
}

pub async fn gmail_address(g: &G<'_>) -> Result<String, String> {
    let v = call(g, M::GET, url(g, "gmail", "/users/me/profile"), None).await?;
    Ok(v["emailAddress"].as_str().unwrap_or("").to_string())
}

fn task_json(t: &Value) -> Value {
    json!({
        "id": t["id"], "title": t["title"].as_str().unwrap_or(""), "notes": t["notes"],
        // Google Tasks stores only the date.
        "due": t["due"].as_str().map(|d| d.chars().take(10).collect::<String>()),
        "completed": t["status"] == "completed", "completed_at": t["completed"], "parent": t["parent"], "position": t["position"],
    })
}

/// Every task list with its tasks (open ones, plus completed when asked).
pub async fn tasks_all(g: &G<'_>, include_completed: bool) -> Result<Vec<Value>, String> {
    let lists = call(g, M::GET, url(g, "tasks", "/users/@me/lists?maxResults=100"), None).await?;
    let lists: Vec<Value> = lists["items"].as_array().cloned().unwrap_or_default();
    let fetch = lists.iter().map(|l| {
        let id = urlencode(l["id"].as_str().unwrap_or(""));
        call(g, M::GET, url(g, "tasks", &format!("/lists/{id}/tasks?maxResults=100&showCompleted={include_completed}&showHidden={include_completed}")), None)
    });
    let mut out = Vec::new();
    for (l, r) in lists.iter().zip(futures_util::future::join_all(fetch).await) {
        let mut tasks: Vec<Value> = r?["items"].as_array().into_iter().flatten().map(task_json).collect();
        tasks.sort_by(|a, b| a["position"].as_str().cmp(&b["position"].as_str()));
        out.push(json!({ "id": l["id"], "title": l["title"], "tasks": tasks }));
    }
    Ok(out)
}

fn task_body(args: &Value) -> Value {
    let mut t = json!({});
    for k in ["title", "notes"] {
        if let Some(v) = args.get(k).and_then(|v| v.as_str()) {
            t[k] = json!(v);
        }
    }
    match args.get("due") {
        Some(Value::String(d)) if d.len() >= 10 => t["due"] = json!(format!("{}T00:00:00.000Z", &d[..10])),
        Some(Value::Null) => t["due"] = Value::Null,
        _ => {}
    }
    if let Some(done) = args.get("completed").and_then(|v| v.as_bool()) {
        t["status"] = json!(if done { "completed" } else { "needsAction" });
        if !done {
            t["completed"] = Value::Null;
        }
    }
    t
}

/// `list_id` defaults to the user's first list.
pub async fn task_create(g: &G<'_>, list_id: Option<&str>, args: &Value) -> Result<Value, String> {
    arg_str(args, "title")?;
    let list = urlencode(list_id.filter(|l| !l.is_empty()).unwrap_or("@default"));
    Ok(task_json(&call(g, M::POST, url(g, "tasks", &format!("/lists/{list}/tasks")), Some(&task_body(args))).await?))
}

pub async fn task_update(g: &G<'_>, list_id: &str, id: &str, args: &Value) -> Result<Value, String> {
    let u = url(g, "tasks", &format!("/lists/{}/tasks/{}", urlencode(list_id), urlencode(id)));
    Ok(task_json(&call(g, M::PATCH, u, Some(&task_body(args))).await?))
}

pub async fn task_delete(g: &G<'_>, list_id: &str, id: &str) -> Result<(), String> {
    call(g, M::DELETE, url(g, "tasks", &format!("/lists/{}/tasks/{}", urlencode(list_id), urlencode(id))), None).await.map(|_| ())
}

// ------------------------------------------------------------------ tool entry

pub async fn execute(app: &App, skill: &Skill, name: &str, args: &Value) -> Result<String, String> {
    let g = &G::new(app, skill);
    let s = |k: &str| args.get(k).and_then(|v| v.as_str());
    match name {
        "gmail_search" => {
            let max = args.get("max").and_then(|v| v.as_u64()).unwrap_or(10).clamp(1, 25) as u32;
            let (list, _) = gmail_list(g, arg_str(args, "query")?, max, None).await?;
            Ok(untrusted("gmail", &serde_json::to_string_pretty(&list).unwrap_or_default()))
        }
        "gmail_read" => {
            let id = arg_str(args, "id")?;
            let d = call(g, M::GET, url(g, "gmail", &format!("/users/me/messages/{}?format=full", urlencode(id))), None).await?;
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
            let id = gmail_send(g, arg_str(args, "to")?, s("cc"), arg_str(args, "subject")?, arg_str(args, "body")?, None).await?;
            Ok(format!("sent (id {id})"))
        }
        "calendar_list" => {
            let now = crate::util::now_ms();
            let min = s("time_min").map(str::to_string).unwrap_or_else(|| iso(now));
            let max = s("time_max").map(str::to_string).unwrap_or_else(|| iso(now + 7 * 86_400_000));
            let items = calendar_events(g, &min, &max, s("query"), 50).await?;
            Ok(untrusted("calendar", &serde_json::to_string_pretty(&items).unwrap_or_default()))
        }
        "calendar_create" => {
            let e = calendar_create(g, args).await?;
            Ok(format!("created event {} ({})", e["id"].as_str().unwrap_or("?"), e["link"].as_str().unwrap_or("")))
        }
        "calendar_delete" => {
            let id = arg_str(args, "event_id")?;
            calendar_delete(g, id).await?;
            Ok(format!("deleted event {id}"))
        }
        "tasks_list" => {
            let lists = tasks_all(g, args.get("include_completed").and_then(|v| v.as_bool()).unwrap_or(false)).await?;
            Ok(untrusted("tasks", &serde_json::to_string_pretty(&lists).unwrap_or_default()))
        }
        "tasks_create" => {
            let t = task_create(g, s("list_id"), args).await?;
            Ok(format!("created task {}", t["id"].as_str().unwrap_or("?")))
        }
        "tasks_complete" => {
            let done = args.get("completed").and_then(|v| v.as_bool()).unwrap_or(true);
            task_update(g, arg_str(args, "list_id")?, arg_str(args, "task_id")?, &json!({ "completed": done })).await?;
            Ok(if done { "completed".into() } else { "reopened".into() })
        }
        "drive_search" => {
            let qs = arg_str(args, "query")?.replace('\\', "\\\\").replace('\'', "\\'");
            let q = format!("(name contains '{qs}' or fullText contains '{qs}') and trashed = false");
            let v = call(g, M::GET, url(g, "drive", &format!("/files?pageSize=20&fields=files(id,name,mimeType,modifiedTime,webViewLink)&q={}", urlencode(&q))), None).await?;
            Ok(untrusted("drive", &serde_json::to_string_pretty(&v["files"]).unwrap_or_default()))
        }
        "drive_read" => {
            let id = urlencode(arg_str(args, "file_id")?);
            let meta = call(g, M::GET, url(g, "drive", &format!("/files/{id}?fields=id,name,mimeType,size")), None).await?;
            let mime = meta["mimeType"].as_str().unwrap_or("");
            let body = match mime {
                "application/vnd.google-apps.document" | "application/vnd.google-apps.presentation" => {
                    call_text(g, url(g, "drive", &format!("/files/{id}/export?mimeType=text%2Fplain"))).await?
                }
                "application/vnd.google-apps.spreadsheet" => call_text(g, url(g, "drive", &format!("/files/{id}/export?mimeType=text%2Fcsv"))).await?,
                m if m.starts_with("text/") || m.contains("json") || m.contains("xml") => call_text(g, url(g, "drive", &format!("/files/{id}?alt=media"))).await?,
                m => return Err(format!("cannot read {m} files as text")),
            };
            Ok(untrusted(&format!("drive:{}", meta["name"].as_str().unwrap_or("")), crate::util::truncate(&body, 150_000)))
        }
        "docs_create" => {
            let d = call(g, M::POST, url(g, "docs", "/documents"), Some(&json!({ "title": arg_str(args, "title")? }))).await?;
            let id = d["documentId"].as_str().unwrap_or("").to_string();
            if let Some(c) = s("content").filter(|c| !c.is_empty()) {
                let req = json!({ "requests": [{ "insertText": { "location": { "index": 1 }, "text": c } }] });
                call(g, M::POST, url(g, "docs", &format!("/documents/{}:batchUpdate", urlencode(&id))), Some(&req)).await?;
            }
            Ok(format!("created document {id} (https://docs.google.com/document/d/{id}/edit)"))
        }
        "docs_append" => {
            let id = arg_str(args, "document_id")?;
            let req = json!({ "requests": [{ "insertText": { "endOfSegmentLocation": {}, "text": arg_str(args, "text")? } }] });
            call(g, M::POST, url(g, "docs", &format!("/documents/{}:batchUpdate", urlencode(id))), Some(&req)).await?;
            Ok(format!("appended to {id}"))
        }
        "sheets_read" => {
            let (id, range) = (urlencode(arg_str(args, "spreadsheet_id")?), urlencode(arg_str(args, "range")?));
            let v = call(g, M::GET, url(g, "sheets", &format!("/spreadsheets/{id}/values/{range}")), None).await?;
            Ok(untrusted("sheets", &serde_json::to_string(&v["values"]).unwrap_or_default()))
        }
        "sheets_update" | "sheets_append" => {
            let (id, range) = (urlencode(arg_str(args, "spreadsheet_id")?), urlencode(arg_str(args, "range")?));
            let values = args.get("values").filter(|v| v.is_array()).ok_or("values must be an array of rows")?;
            let body = json!({ "values": values });
            if name == "sheets_update" {
                let r = call(g, M::PUT, url(g, "sheets", &format!("/spreadsheets/{id}/values/{range}?valueInputOption=USER_ENTERED")), Some(&body)).await?;
                Ok(format!("updated {} cells", r["updatedCells"].as_u64().unwrap_or(0)))
            } else {
                let r = call(g, M::POST, url(g, "sheets", &format!("/spreadsheets/{id}/values/{range}:append?valueInputOption=USER_ENTERED&insertDataOption=INSERT_ROWS")), Some(&body)).await?;
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
        let raw = build_email("a@example.com", None, "こんにちは", "本文", None).unwrap();
        let m = String::from_utf8(b64_decode(&raw).unwrap()).unwrap();
        assert!(m.starts_with("To: a@example.com\r\n"));
        assert!(m.contains("Subject: =?UTF-8?B?"));
        assert!(build_email("a@example.com\r\nBcc: evil@example.com", None, "s", "b", None).is_err());
        assert!(build_email("a@example.com", None, "s\nBcc: x", "b", None).is_err());
    }

    #[test]
    fn redirect_parsing() {
        assert_eq!(
            crate::skills::auth::parse_redirect("http://127.0.0.1:7878/oauth/google/callback?state=abc&code=4%2F0Ax&scope=x").unwrap(),
            ("4/0Ax".to_string(), "abc".to_string())
        );
    }
}
