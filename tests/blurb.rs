//! One-line comments for the app screens.

mod common;

use common::Harness;
use serde_json::json;

#[tokio::test]
async fn screens_get_a_line_from_alice() {
    let h = Harness::new().await;
    let r = h.post("/blurb", json!({ "screen": "mail", "lang": "ja", "facts": "unread: 3 (Yuki: lunch?)", "now": "2026-11-26 09:00" })).await;
    let text = r["text"].as_str().unwrap();
    assert!(text.contains("unread: 3"), "{text}");
    // The same facts are answered from the cache.
    let again = h.post("/blurb", json!({ "screen": "mail", "lang": "ja", "facts": "unread: 3 (Yuki: lunch?)", "now": "2026-11-26 09:20" })).await;
    assert_eq!(again["text"], r["text"]);
    let (code, _) = h.req(reqwest::Method::POST, "/blurb", Some(json!({ "screen": "nope" }))).await;
    assert_eq!(code, 400);
    // The tone is a setting.
    let (code, cfg) = h.req(reqwest::Method::PATCH, "/config", Some(json!({ "agent.blurb_tone": "執事風" }))).await;
    assert_eq!((code, cfg["agent"]["blurb_tone"].as_str()), (200, Some("執事風")));
}
