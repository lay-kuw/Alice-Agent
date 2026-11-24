//! Built-in handler for the google-workspace skill (Gmail, Calendar, Drive,
//! Docs, Sheets). Tool definitions, scopes and OAuth live in the skill
//! (`skills/google-workspace`); this module only talks to the Google APIs.

use super::{arg_str, untrusted};
use crate::app::App;
use crate::skills::Skill;
use crate::util::{b64_decode, b64_encode, b64url_encode, urlencode};
use serde_json::{Value, json};
use std::time::Duration;

/// Request context: the skill (for its token) and an optional API base override.
struct G<'a> {
    app: &'a App,
    skill: &'a Skill,
    base: Option<String>,
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
        _ => "https://sheets.googleapis.com/v4",
    };
    format!("{base}{path}")
}

async fn call(g: &G<'_>, method: reqwest::Method, u: String, body: Option<&Value>) -> Result<Value, String> {
    let token = crate::skills::auth::access_token(g.app, g.skill).await?;
    let mut rb = g.app.http.request(method, u).bearer_auth(token).timeout(Duration::from_secs(30));
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

pub async fn execute(app: &App, skill: &Skill, name: &str, args: &Value) -> Result<String, String> {
    let cfg = app.config();
    let g = &G { app, skill, base: crate::skills::setting(app, &cfg, skill, "api_base") };
    use reqwest::Method as M;
    let s = |k: &str| args.get(k).and_then(|v| v.as_str());
    match name {
        "gmail_search" => {
            let max = args.get("max").and_then(|v| v.as_u64()).unwrap_or(10).clamp(1, 25);
            let list = call(g, M::GET, url(g, "gmail", &format!("/users/me/messages?q={}&maxResults={max}", urlencode(arg_str(args, "query")?))), None).await?;
            let mut out = Vec::new();
            for m in list["messages"].as_array().into_iter().flatten() {
                let id = m["id"].as_str().unwrap_or("");
                let d = call(g, M::GET, url(g, "gmail", &format!("/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date", urlencode(id))), None).await?;
                let h = &d["payload"]["headers"];
                out.push(json!({ "id": id, "from": header(h, "From"), "subject": header(h, "Subject"), "date": header(h, "Date"), "snippet": d["snippet"] }));
            }
            Ok(untrusted("gmail", &serde_json::to_string_pretty(&out).unwrap_or_default()))
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
            let raw = build_email(arg_str(args, "to")?, s("cc"), arg_str(args, "subject")?, arg_str(args, "body")?)?;
            let r = call(g, M::POST, url(g, "gmail", "/users/me/messages/send"), Some(&json!({ "raw": raw }))).await?;
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
            let v = call(g, M::GET, url(g, "calendar", &q), None).await?;
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
            let r = call(g, M::POST, url(g, "calendar", "/calendars/primary/events"), Some(&ev)).await?;
            Ok(format!("created event {} ({})", r["id"].as_str().unwrap_or("?"), r["htmlLink"].as_str().unwrap_or("")))
        }
        "calendar_delete" => {
            let id = arg_str(args, "event_id")?;
            call(g, M::DELETE, url(g, "calendar", &format!("/calendars/primary/events/{}", urlencode(id))), None).await?;
            Ok(format!("deleted event {id}"))
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
        let raw = build_email("a@example.com", None, "こんにちは", "本文").unwrap();
        let m = String::from_utf8(b64_decode(&raw).unwrap()).unwrap();
        assert!(m.starts_with("To: a@example.com\r\n"));
        assert!(m.contains("Subject: =?UTF-8?B?"));
        assert!(build_email("a@example.com\r\nBcc: evil@example.com", None, "s", "b").is_err());
        assert!(build_email("a@example.com", None, "s\nBcc: x", "b").is_err());
    }

    #[test]
    fn redirect_parsing() {
        assert_eq!(
            crate::skills::auth::parse_redirect("http://127.0.0.1:7878/oauth/google/callback?state=abc&code=4%2F0Ax&scope=x").unwrap(),
            ("4/0Ax".to_string(), "abc".to_string())
        );
    }
}
