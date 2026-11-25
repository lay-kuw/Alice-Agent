//! Scheduled agents: due instructions are posted into their thread and answered.

mod common;

use common::Harness;
use serde_json::json;

#[tokio::test]
async fn due_agents_post_into_their_thread() {
    let h = Harness::new().await;
    let now = alice::util::now_ms();

    // Bad input is refused.
    let (code, _) = h.req(reqwest::Method::POST, "/agents", Some(json!({ "name": "x", "prompt": "hi", "every_min": 1 }))).await;
    assert_eq!(code, 400);

    // A repeating agent without a thread, due a minute ago.
    let a = h.post("/agents", json!({ "name": "Morning news", "prompt": "summarize the news", "start_at": now - 60_000, "every_min": 60 })).await["agent"].clone();
    let id = a["id"].as_str().unwrap().to_string();
    // Created in the past: the first run is the next slot on its grid.
    assert_eq!(a["next_run"], now - 60_000 + 3_600_000);

    // A one-off into an existing thread, due now.
    let t = h.new_thread().await;
    let once = h.post("/agents", json!({ "name": "Remind", "prompt": "remind me to stretch", "thread_id": t, "start_at": now - 1000 })).await["agent"].clone();
    assert_eq!(once["next_run"], now - 1000);

    let mut ev = h.events(Some(&t)).await;
    assert_eq!(alice::schedule::tick(&h.app, now), 1);
    let reply = ev.until_turn_done().await;
    assert!(reply["content"].as_str().unwrap().contains("remind me to stretch"), "{reply}");
    let msgs = h.get(&format!("/threads/{t}/messages")).await;
    let user = msgs["messages"].as_array().unwrap().iter().find(|m| m["role"] == "user").unwrap().clone();
    assert_eq!((user["content"].as_str(), user["client"].as_str()), (Some("remind me to stretch"), Some("agent")));

    // The one-off is done; nothing is due any more.
    let list = h.get("/agents").await;
    let o = list["agents"].as_array().unwrap().iter().find(|x| x["name"] == "Remind").unwrap().clone();
    assert!(o["next_run"].is_null() && o["last_status"] == "ok", "{o}");
    assert_eq!(alice::schedule::tick(&h.app, now + 1000), 0);

    // At its slot the repeating agent makes its own thread and keeps it.
    assert_eq!(alice::schedule::tick(&h.app, now - 60_000 + 3_600_000), 1);
    let a = h.get("/agents").await["agents"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap().clone();
    let tid = a["thread_id"].as_str().unwrap().to_string();
    assert_eq!(a["thread_title"], "Morning news");
    assert_eq!(a["next_run"], now - 60_000 + 2 * 3_600_000);

    // Run now leaves the schedule alone (wait for the first run to finish first).
    for _ in 0..100 {
        if h.get(&format!("/threads/{tid}")).await["generating"] == false {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let r = h.post(&format!("/agents/{id}/run"), json!({})).await;
    assert_eq!(r["thread_id"], tid);
    let a2 = h.get("/agents").await["agents"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap().clone();
    assert_eq!(a2["next_run"], a["next_run"]);

    // Edit and delete.
    let (code, v) = h.req(reqwest::Method::PATCH, &format!("/agents/{id}"), Some(json!({ "enabled": false }))).await;
    assert_eq!((code, v["agent"]["enabled"].as_bool()), (200, Some(false)));
    assert_eq!(h.delete(&format!("/agents/{id}"), None).await, 204);
    assert_eq!(h.delete(&format!("/agents/{id}"), None).await, 404);
}
