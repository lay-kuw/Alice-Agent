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
    assert!(auth_url.contains("gmail.readonly") && auth_url.contains("calendar.events"));
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
