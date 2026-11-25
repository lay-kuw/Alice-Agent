//! Google OAuth (PKCE, loopback) and tools against a fake Google API.

mod common;

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use common::Harness;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FakeGoogle {
    token_requests: Mutex<Vec<HashMap<String, String>>>,
    sent: Mutex<Vec<Value>>,
    events: Mutex<Vec<Value>>,
    revoked: Mutex<bool>,
}

async fn token(State(st): State<Arc<FakeGoogle>>, Form(f): Form<HashMap<String, String>>) -> Json<Value> {
    st.token_requests.lock().unwrap().push(f.clone());
    match f["grant_type"].as_str() {
        "authorization_code" => Json(json!({ "access_token": "at-1", "expires_in": 3600, "refresh_token": "rt-1" })),
        _ => Json(json!({ "access_token": "at-2", "expires_in": 3600 })),
    }
}

fn authed(h: &axum::http::HeaderMap) {
    let a = h["authorization"].to_str().unwrap();
    assert!(a == "Bearer at-1" || a == "Bearer at-2", "{a}");
}

async fn gmail_list(h: axum::http::HeaderMap, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    authed(&h);
    assert_eq!(q["q"], "from:bob");
    Json(json!({ "messages": [{ "id": "m1" }] }))
}

async fn gmail_get(h: axum::http::HeaderMap) -> Json<Value> {
    authed(&h);
    Json(json!({ "id": "m1", "snippet": "lunch?", "payload": { "headers": [
        { "name": "From", "value": "Bob <bob@example.com>" }, { "name": "Subject", "value": "Lunch tomorrow" }, { "name": "Date", "value": "Mon" } ] } }))
}

async fn gmail_send(State(st): State<Arc<FakeGoogle>>, h: axum::http::HeaderMap, Json(v): Json<Value>) -> Json<Value> {
    authed(&h);
    st.sent.lock().unwrap().push(v);
    Json(json!({ "id": "sent-1" }))
}

async fn cal_create(State(st): State<Arc<FakeGoogle>>, h: axum::http::HeaderMap, Json(v): Json<Value>) -> Json<Value> {
    authed(&h);
    st.events.lock().unwrap().push(v);
    Json(json!({ "id": "ev1", "htmlLink": "https://calendar/ev1" }))
}

async fn revoke(State(st): State<Arc<FakeGoogle>>) -> Json<Value> {
    *st.revoked.lock().unwrap() = true;
    Json(json!({}))
}

#[tokio::test]
async fn google_oauth_pkce_and_tools() {
    let fake = Arc::new(FakeGoogle::default());
    let router = Router::new()
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
        .route("/gmail/users/me/messages", get(gmail_list))
        .route("/gmail/users/me/messages/m1", get(gmail_get))
        .route("/gmail/users/me/messages/send", post(gmail_send))
        .route("/calendar/calendars/primary/events", post(cal_create))
        .with_state(fake.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });

    let h = Harness::with(|c, s| {
        c.tools.google.client_id = "cid.apps.googleusercontent.com".into();
        c.tools.google.services = vec!["gmail".into(), "calendar".into()];
        c.tools.google.api_base = base.clone();
        s.set("google.client_secret", "csecret");
    })
    .await;

    // Not connected: Google tools are not offered to the model.
    let tools = h.get("/tools").await;
    let avail = |v: &Value, n: &str| v["tools"].as_array().unwrap().iter().find(|t| t["name"] == n).unwrap()["available"].clone();
    assert_eq!(avail(&tools, "gmail_search"), false);

    // Start login; only scopes of enabled services are requested.
    let r = h.post("/skills/google-workspace/login", json!({})).await;
    let auth_url = r["auth_url"].as_str().unwrap().to_string();
    assert!(auth_url.contains("gmail.readonly") && auth_url.contains("gmail.modify") && auth_url.contains("calendar.events"));
    assert!(!auth_url.contains("drive") && !auth_url.contains("spreadsheets"));
    assert!(auth_url.contains("code_challenge_method=S256"));
    let param = |k: &str| auth_url.split(&format!("{k}=")).nth(1).unwrap().split('&').next().unwrap().to_string();
    let (state, challenge) = (param("state"), param("code_challenge"));

    // Wrong state is rejected; the browser redirect completes the login.
    let page = reqwest::get(format!("{}/oauth/callback?code=c1&state=wrong", h.base)).await.unwrap().text().await.unwrap();
    assert!(page.contains("Could not connect"), "{page}");
    let r = h.post("/skills/google-workspace/login", json!({})).await;
    let auth_url = r["auth_url"].as_str().unwrap().to_string();
    let param = |k: &str| auth_url.split(&format!("{k}=")).nth(1).unwrap().split('&').next().unwrap().to_string();
    let (state, challenge) = { let _ = (state, challenge); (param("state"), param("code_challenge")) };
    let page = reqwest::get(format!("{}/oauth/callback?code=c1&state={state}", h.base)).await.unwrap().text().await.unwrap();
    assert!(page.contains("Connected (google-workspace)"), "{page}");
    // The state is single-use.
    let again = reqwest::get(format!("{}/oauth/callback?code=c1&state={state}", h.base)).await.unwrap().text().await.unwrap();
    assert!(again.contains("Could not connect"), "{again}");
    let tr = fake.token_requests.lock().unwrap()[0].clone();
    assert_eq!(tr["code"], "c1");
    assert_eq!(tr["client_secret"], "csecret");
    let digest = alice::util::b64url_encode(&Sha256::digest(tr["code_verifier"].as_bytes()));
    assert_eq!(digest, challenge, "PKCE verifier must match the challenge");
    let connected = |v: Value| v["skills"].as_array().unwrap().iter().find(|s| s["id"] == "google-workspace").unwrap()["auth"]["connected"].clone();
    assert_eq!(connected(h.get("/skills").await), true);
    // Legacy settings were migrated; secrets never appear in API responses.
    let skills = h.get("/skills").await.to_string();
    assert!(skills.contains("cid.apps.googleusercontent.com") && !skills.contains("csecret") && !skills.contains("rt-1"));
    assert!(std::fs::read_to_string(h.paths.secrets()).unwrap().contains("rt-1"));
    assert!(!h.get("/config").await.to_string().contains("rt-1"));

    // Read-only tool runs directly; results are untrusted data.
    let t = h.new_thread().await;
    let r = h.chat(&t, r#"calljson {"name":"gmail_search","arguments":{"query":"from:bob"}}"#, "cli").await;
    let c = r["content"].as_str().unwrap();
    assert!(c.contains("Lunch tomorrow") && c.contains("<untrusted"), "{c}");

    // Sending mail asks first; approve and check the raw message.
    let mut ev = h.events(Some(&t)).await;
    h.post(&format!("/threads/{t}/messages"), json!({ "content": r#"calljson {"name":"gmail_send","arguments":{"to":"bob@example.com","subject":"Re: Lunch","body":"Yes!"}}"# })).await;
    let ask = ev.until(|v| v["type"] == "tool.approval_required").await;
    assert_eq!(ask["tool"], "gmail_send");
    h.post(&format!("/approvals/{}", ask["approval_id"].as_str().unwrap()), json!({ "approve": true })).await;
    ev.until_turn_done().await;
    let raw = fake.sent.lock().unwrap()[0]["raw"].as_str().unwrap().to_string();
    let mail = String::from_utf8(alice::util::b64_decode(&raw).unwrap()).unwrap();
    assert!(mail.starts_with("To: bob@example.com\r\nSubject: Re: Lunch"), "{mail}");

    // Calendar create (approved) with all-day dates.
    h.req(reqwest::Method::PATCH, "/tools", Some(json!({ "calendar_create": "allow" }))).await;
    h.chat(&t, r#"calljson {"name":"calendar_create","arguments":{"summary":"Trip","start":"2026-12-01","end":"2026-12-03"}}"#, "web").await;
    let e = fake.events.lock().unwrap()[0].clone();
    assert_eq!(e["start"]["date"], "2026-12-01");

    // Disabled service: drive tools unavailable even when connected.
    assert_eq!(avail(&h.get("/tools").await, "drive_search"), false);

    // Disconnect revokes and forgets the refresh token.
    // The run log has the tool calls (without arguments or results).
    let runs = h.get("/tool-runs").await.to_string();
    assert!(runs.contains("gmail_send") && runs.contains("calendar_create") && !runs.contains("Re: Lunch"), "{runs}");

    h.post("/skills/google-workspace/logout", json!({})).await;
    assert!(*fake.revoked.lock().unwrap());
    assert!(!std::fs::read_to_string(h.paths.secrets()).unwrap().contains("rt-1"));
    assert_eq!(connected(h.get("/skills").await), false);
}

// ------------------------------------------------------------------ app API

#[derive(Default)]
struct FakeApp {
    sent: Mutex<Vec<Value>>,
    tasks: Mutex<Vec<Value>>,
    modified: Mutex<Vec<(String, Value)>>,
}

async fn app_event_write(State(st): State<Arc<FakeApp>>, Json(v): Json<Value>) -> Json<Value> {
    st.modified.lock().unwrap().push(("event".into(), v.clone()));
    let mut e = v;
    e["id"] = json!("e9");
    e["start"] = json!({ "dateTime": "2026-11-26T10:00:00+09:00" });
    e["end"] = json!({ "dateTime": "2026-11-26T11:00:00+09:00" });
    Json(e)
}

async fn app_modify(State(st): State<Arc<FakeApp>>, axum::extract::Path(id): axum::extract::Path<String>, Json(v): Json<Value>) -> Json<Value> {
    st.modified.lock().unwrap().push((id.clone(), v));
    Json(json!({ "id": id }))
}

async fn app_events(h: axum::http::HeaderMap, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    authed(&h);
    assert_eq!(q["timeMin"], "2026-11-23T00:00:00+09:00");
    Json(json!({ "items": [
        { "id": "e1", "summary": "歯医者", "start": { "dateTime": "2026-11-25T10:00:00+09:00" }, "end": { "dateTime": "2026-11-25T11:00:00+09:00" } },
        { "id": "e2", "summary": "Trip", "start": { "date": "2026-11-28" }, "end": { "date": "2026-11-29" } } ] }))
}

async fn app_mail_list(h: axum::http::HeaderMap, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    authed(&h);
    assert_eq!(q["q"], "in:inbox");
    Json(json!({ "messages": [{ "id": "m1" }, { "id": "m2" }], "nextPageToken": "p2" }))
}

async fn app_mail_get(h: axum::http::HeaderMap, axum::extract::Path(id): axum::extract::Path<String>) -> Json<Value> {
    authed(&h);
    let body = alice::util::b64url_encode("Lunch at noon?".as_bytes());
    let html = alice::util::b64url_encode("<p>Lunch at <b>noon</b>?</p>".as_bytes());
    // m2 is multipart/alternative with an HTML part.
    let parts = if id == "m2" {
        json!([{ "mimeType": "text/plain", "body": { "data": body } }, { "mimeType": "text/html", "body": { "data": html } }])
    } else {
        json!([])
    };
    Json(json!({ "id": id, "threadId": "th1", "snippet": "it&#39;s lunch", "internalDate": "1795000000000", "labelIds": if id == "m1" { json!(["INBOX", "UNREAD"]) } else { json!(["INBOX"]) },
        "payload": { "mimeType": if id == "m2" { "multipart/alternative" } else { "text/plain" }, "body": if id == "m2" { json!({}) } else { json!({ "data": body }) }, "parts": parts, "headers": [
            { "name": "From", "value": "Bob <bob@example.com>" }, { "name": "Subject", "value": "Lunch" }, { "name": "Message-ID", "value": "<abc@mail>" } ] } }))
}

async fn app_send(State(st): State<Arc<FakeApp>>, Json(v): Json<Value>) -> Json<Value> {
    st.sent.lock().unwrap().push(v);
    Json(json!({ "id": "s1" }))
}

async fn app_token() -> Json<Value> {
    Json(json!({ "access_token": "at-1", "expires_in": 3600, "refresh_token": "rt-1" }))
}

async fn app_profile() -> Json<Value> {
    Json(json!({ "emailAddress": "me@example.com" }))
}

async fn app_lists() -> Json<Value> {
    Json(json!({ "items": [{ "id": "L1", "title": "My Tasks" }] }))
}

async fn app_tasks(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    assert_eq!(q["showCompleted"], "true");
    Json(json!({ "items": [
        { "id": "t2", "title": "牛乳", "status": "needsAction", "position": "2" },
        { "id": "t1", "title": "Buy flowers", "status": "completed", "due": "2026-11-25T00:00:00.000Z", "position": "1" } ] }))
}

async fn app_task_write(State(st): State<Arc<FakeApp>>, Json(v): Json<Value>) -> Json<Value> {
    st.tasks.lock().unwrap().push(v.clone());
    let mut t = v;
    t["id"] = json!("t9");
    Json(t)
}

#[tokio::test]
async fn app_api_for_calendar_mail_and_tasks() {
    let fake = Arc::new(FakeApp::default());
    let router = Router::new()
        .route("/oauth/token", post(app_token))
        .route("/calendar/calendars/primary/events", get(app_events).post(app_event_write))
        .route("/calendar/calendars/primary/events/{id}", axum::routing::patch(app_event_write))
        .route("/gmail/users/me/messages", get(app_mail_list))
        .route("/gmail/users/me/messages/send", post(app_send))
        .route("/gmail/users/me/messages/{id}", get(app_mail_get))
        .route("/gmail/users/me/messages/{id}/modify", post(app_modify))
        .route("/gmail/users/me/profile", get(app_profile))
        .route("/tasks/users/@me/lists", get(app_lists))
        .route("/tasks/lists/L1/tasks", get(app_tasks).post(app_task_write))
        .route("/tasks/lists/L1/tasks/t2", axum::routing::patch(app_task_write))
        .with_state(fake.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    let h = Harness::with(|c, s| {
        c.tools.google.client_id = "cid".into();
        c.tools.google.services = vec!["gmail".into(), "calendar".into()];
        c.tools.google.api_base = base.clone();
        s.set("google.client_secret", "csecret");
    })
    .await;

    // Not connected yet.
    let (code, v) = h.req(reqwest::Method::GET, "/google/tasks", None).await;
    assert_eq!(code, 409, "{v}");
    let r = h.post("/skills/google-workspace/login", json!({})).await;
    let state = r["auth_url"].as_str().unwrap().split("state=").nth(1).unwrap().split('&').next().unwrap().to_string();
    reqwest::get(format!("{}/oauth/callback?code=c1&state={state}", h.base)).await.unwrap();

    let st = h.get("/google").await;
    assert_eq!(st["status"], "ready");
    assert_eq!(st["email"], "me@example.com");
    assert_eq!(st["groups"], json!({ "calendar": true, "gmail": true, "tasks": false }));

    // Calendar.
    let v = h.get("/google/calendar/events?from=2026-11-23T00:00:00%2B09:00&to=2026-11-30T00:00:00%2B09:00").await;
    let ev = v["events"].as_array().unwrap();
    assert_eq!(ev[0]["summary"], "歯医者");
    assert_eq!(ev[0]["all_day"], false);
    assert_eq!(ev[1]["all_day"], true);
    assert_eq!(ev[1]["start"], "2026-11-28");
    // Color tags: on create and changed later; out of range is refused.
    let v = h.post("/google/calendar/events", json!({ "summary": "Yoga", "start": "2026-11-26T10:00:00+09:00", "end": "2026-11-26T11:00:00+09:00", "color_id": "2" })).await;
    assert_eq!(v["event"]["color_id"], "2");
    let (code, v) = h.req(reqwest::Method::PATCH, "/google/calendar/events/e9", Some(json!({ "color_id": "11" }))).await;
    assert_eq!((code, v["event"]["color_id"].clone()), (200, json!("11")));
    h.req(reqwest::Method::PATCH, "/google/calendar/events/e9", Some(json!({ "color_id": "" }))).await;
    let (code, _) = h.req(reqwest::Method::PATCH, "/google/calendar/events/e9", Some(json!({ "color_id": "12" }))).await;
    assert_ne!(code, 200);
    let w: Vec<Value> = fake.modified.lock().unwrap().drain(..).map(|(_, v)| v).collect();
    assert_eq!(w[0]["colorId"], "2");
    assert_eq!(w[1], json!({ "colorId": "11" }));
    assert_eq!(w[2], json!({ "colorId": null }));
    assert_eq!(w.len(), 3);

    // Mail: list with unread flags, read, reply in thread.
    let v = h.get("/google/mail").await;
    assert_eq!(v["next_page"], "p2");
    let m = v["messages"].as_array().unwrap();
    assert_eq!((m[0]["unread"].clone(), m[1]["unread"].clone()), (json!(true), json!(false)));
    assert_eq!(m[0]["snippet"], "it's lunch");
    assert_eq!(h.get("/google/mail/m1").await["message"]["body"], "Lunch at noon?");
    assert!(h.get("/google/mail/m1").await["message"]["html"].is_null());
    let m2 = h.get("/google/mail/m2").await;
    assert_eq!(m2["message"]["body"], "Lunch at noon?");
    assert_eq!(m2["message"]["html"], "<p>Lunch at <b>noon</b>?</p>");

    // The Web UI's mail frame: the mail's own HTML under a locked-down CSP.
    let page = |q: &'static str| {
        let url = h.url(&format!("/google/mail/m2/html{q}"));
        let req = h.http.get(url).bearer_auth(&h.token);
        async move { req.send().await.unwrap() }
    };
    let r = page("").await;
    let csp = r.headers()["content-security-policy"].to_str().unwrap().to_string();
    assert!(csp.contains("sandbox") && csp.contains("img-src data: cid:;"), "{csp}");
    assert!(!csp.contains("script-src") && csp.starts_with("default-src 'none'"));
    assert_eq!(r.headers()["x-frame-options"], "SAMEORIGIN");
    assert!(r.text().await.unwrap().contains("<p>Lunch at <b>noon</b>?</p>"));
    let r = page("?images=1").await;
    assert!(r.headers()["content-security-policy"].to_str().unwrap().contains("img-src data: cid: https: http:"));
    // Plain-text mail is escaped into a <pre>.
    let r = h.http.get(h.url("/google/mail/m1/html")).bearer_auth(&h.token).send().await.unwrap();
    assert!(r.text().await.unwrap().contains("<pre class=\"alice-plain\">Lunch at noon?</pre>"));
    // Everything else keeps the app's own policy.
    let r = h.http.get(h.url("/google/mail/m1")).bearer_auth(&h.token).send().await.unwrap();
    assert_eq!(r.headers()["x-frame-options"], "DENY");
    h.post("/google/mail/send", json!({ "to": "bob@example.com", "subject": "Re: Lunch", "body": "Yes!", "reply_to_id": "m1" })).await;
    let sent = fake.sent.lock().unwrap()[0].clone();
    assert_eq!(sent["threadId"], "th1");
    let raw = String::from_utf8(alice::util::b64_decode(sent["raw"].as_str().unwrap()).unwrap()).unwrap();
    assert!(raw.contains("In-Reply-To: <abc@mail>\r\nReferences: <abc@mail>\r\n"), "{raw}");
    // Swipe to mark read.
    h.post("/google/mail/m1/read", json!({})).await;
    h.post("/google/mail/m1/read", json!({ "read": false })).await;
    let md = fake.modified.lock().unwrap().clone();
    assert_eq!(md, vec![("m1".into(), json!({ "removeLabelIds": ["UNREAD"] })), ("m1".into(), json!({ "addLabelIds": ["UNREAD"] }))]);

    // Tasks are off until the group is turned on.
    let (code, _) = h.req(reqwest::Method::GET, "/google/tasks", None).await;
    assert_eq!(code, 403);
    let (code, _) = h.req(reqwest::Method::PATCH, "/skills/google-workspace", Some(json!({ "groups": ["gmail", "calendar", "tasks"] }))).await;
    assert_eq!(code, 200);
    let v = h.get("/google/tasks").await;
    let tasks = v["lists"][0]["tasks"].as_array().unwrap();
    assert_eq!(tasks[0]["title"], "Buy flowers", "sorted by position");
    assert_eq!(tasks[0]["due"], "2026-11-25");
    assert_eq!(tasks[0]["completed"], true);
    h.post("/google/tasks/L1", json!({ "title": "Call mom", "due": "2026-11-30" })).await;
    h.req(reqwest::Method::PATCH, "/google/tasks/L1/t2", Some(json!({ "completed": true }))).await;
    let w = fake.tasks.lock().unwrap().clone();
    assert_eq!(w[0], json!({ "title": "Call mom", "due": "2026-11-30T00:00:00.000Z" }));
    assert_eq!(w[1], json!({ "status": "completed" }));
}
