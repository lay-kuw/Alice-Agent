//! Scheduled agents: due instructions are answered in their thread, the
//! instruction itself stays hidden, and "nothing to report" leaves no trace.

mod common;

use common::Harness;
use serde_json::{Value, json};

async fn idle(h: &Harness, t: &str) {
    for _ in 0..200 {
        // Done, or gone (a quiet run removes the chat it made).
        let (code, v) = h.req(reqwest::Method::GET, &format!("/threads/{t}"), None).await;
        if code == 404 || v["generating"] == false {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("still generating");
}

async fn agent(h: &Harness, id: &str) -> Value {
    h.get("/agents").await["agents"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap().clone()
}

async fn settle(h: &Harness, id: &str) -> Value {
    for _ in 0..200 {
        let a = agent(h, id).await;
        if a["last_status"] != "running" {
            return a;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("agent still running");
}

#[tokio::test]
async fn due_agents_post_into_their_thread() {
    let h = Harness::new().await;
    let now = alice::util::now_ms();

    let (code, _) = h.req(reqwest::Method::POST, "/agents", Some(json!({ "name": "x", "prompt": "hi", "every_min": 1 }))).await;
    assert_eq!(code, 400);

    // Interval mode runs as soon as possible.
    let t = h.new_thread().await;
    let a = h.post("/agents", json!({ "name": "Stretch", "prompt": "remind me to stretch", "thread_id": t, "every_min": 60 })).await["agent"].clone();
    let id = a["id"].as_str().unwrap().to_string();
    assert!(a["next_run"].as_i64().unwrap() <= alice::util::now_ms());

    let now = alice::util::now_ms().max(now);
    assert_eq!(alice::schedule::tick(&h.app, now), 1);
    idle(&h, &t).await;
    let a = settle(&h, &id).await;
    assert_eq!(a["last_status"], "ok");
    assert_eq!(a["next_run"].as_i64().unwrap(), a["start_at"].as_i64().unwrap() + 3_600_000);

    // The reply is there; the instruction is a user message from "agent" (clients hide it).
    let msgs = h.get(&format!("/threads/{t}/messages")).await["messages"].as_array().unwrap().clone();
    let user = msgs.iter().find(|m| m["role"] == "user").unwrap();
    assert_eq!(user["client"], "agent");
    assert!(user["content"].as_str().unwrap().starts_with("remind me to stretch"));
    let reply = msgs.iter().find(|m| m["role"] == "assistant").unwrap();
    assert!(reply["content"].as_str().unwrap().contains("remind me to stretch"));

    // Nothing to report in an existing chat: the run is removed, earlier messages stay.
    let q = h.post("/agents", json!({ "name": "Mail check", "prompt": "quietrun check mail", "thread_id": t, "every_min": 30 })).await["agent"].clone();
    let qid = q["id"].as_str().unwrap().to_string();
    assert_eq!(alice::schedule::tick(&h.app, alice::util::now_ms()), 1);
    idle(&h, &t).await;
    assert_eq!(settle(&h, &qid).await["last_status"], "quiet");
    let after = h.get(&format!("/threads/{t}/messages")).await["messages"].as_array().unwrap().clone();
    assert_eq!(after.len(), msgs.len(), "{after:?}");

    // A message sent the moment the quiet run lets go of the chat is never swept up with it.
    let mut ev = h.events(Some(&t)).await;
    assert_eq!(alice::schedule::tick(&h.app, alice::util::now_ms() + 31 * 60_000), 1);
    let mut sent = None;
    for _ in 0..400 {
        let (code, v) = h.req(reqwest::Method::POST, &format!("/threads/{t}/messages"), Some(json!({ "content": "right after", "client": "web" }))).await;
        if code == 202 {
            sent = v["user_message"]["id"].as_str().map(str::to_string);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let sent = sent.expect("could not post after the run");
    // Every turn ends with one turn.done, whatever happened.
    let done = ev.until(|v| v["type"] == "turn.done").await;
    assert_eq!(done["thread_id"], t);
    idle(&h, &t).await;
    let all = h.get(&format!("/threads/{t}/messages")).await["messages"].as_array().unwrap().clone();
    assert!(all.iter().any(|m| m["id"] == sent.as_str()), "the later message was removed");

    // Nothing to report in a chat made for the run: the chat goes away too.
    let n = h.post("/agents", json!({ "name": "Quiet one", "prompt": "quietrun news", "every_min": 30 })).await["agent"].clone();
    let nid = n["id"].as_str().unwrap().to_string();
    let threads_before = h.get("/threads").await["threads"].as_array().unwrap().len();
    alice::schedule::tick(&h.app, alice::util::now_ms());
    let made = agent(&h, &nid).await["thread_id"].as_str().map(str::to_string);
    if let Some(m) = made {
        idle(&h, &m).await;
    }
    assert_eq!(settle(&h, &nid).await["last_status"], "quiet");
    assert_eq!(h.get("/threads").await["threads"].as_array().unwrap().len(), threads_before);
    assert!(agent(&h, &nid).await["thread_id"].is_null());

    // Daily times mode.
    let d = h.post("/agents", json!({ "name": "Daily", "prompt": "news", "times": ["17:00", "12:00"] })).await["agent"].clone();
    assert_eq!(d["times"], json!(["12:00", "17:00"]));
    assert!(d["next_run"].as_i64().unwrap() > alice::util::now_ms() - 1000);

    // Reply target: left out keeps it, null clears it (a new chat next time).
    let (_, v) = h.req(reqwest::Method::PATCH, &format!("/agents/{id}"), Some(json!({ "name": "Stretch!" }))).await;
    assert_eq!(v["agent"]["thread_id"], t);
    let (_, v) = h.req(reqwest::Method::PATCH, &format!("/agents/{id}"), Some(json!({ "thread_id": null }))).await;
    assert!(v["agent"]["thread_id"].is_null(), "{v}");
    let (_, v) = h.req(reqwest::Method::PATCH, &format!("/agents/{id}"), Some(json!({ "thread_id": t }))).await;
    assert_eq!(v["agent"]["thread_id"], t);

    // Run now leaves the schedule alone; edit and delete.
    let before = agent(&h, &id).await["next_run"].clone();
    let r = h.post(&format!("/agents/{id}/run"), json!({})).await;
    assert_eq!(r["thread_id"], t);
    idle(&h, &t).await;
    assert_eq!(settle(&h, &id).await["next_run"], before);
    // Run history: newest first, scheduled and manual runs, with their result.
    let runs = h.get(&format!("/agents/{id}/runs")).await["runs"].as_array().unwrap().clone();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!((runs[0]["status"].as_str(), runs[0]["manual"].as_bool()), (Some("ok"), Some(true)));
    assert_eq!((runs[1]["status"].as_str(), runs[1]["manual"].as_bool()), (Some("ok"), Some(false)));
    assert_eq!(runs[1]["thread_id"], t);
    assert!(runs[0]["finished_at"].as_i64().unwrap() >= runs[0]["started_at"].as_i64().unwrap());
    let quiet = h.get(&format!("/agents/{qid}/runs")).await["runs"].as_array().unwrap().clone();
    assert!(quiet.len() == 2 && quiet.iter().all(|r| r["status"] == "quiet"), "{quiet:?}");
    // A quiet run that removed its own chat points nowhere.
    let gone = h.get(&format!("/agents/{nid}/runs")).await["runs"][0].clone();
    assert!(gone["status"] == "quiet" && gone["thread_id"].is_null(), "{gone}");

    let (code, v) = h.req(reqwest::Method::PATCH, &format!("/agents/{id}"), Some(json!({ "enabled": false }))).await;
    assert_eq!((code, v["agent"]["enabled"].as_bool()), (200, Some(false)));
    assert_eq!(h.delete(&format!("/agents/{id}"), None).await, 204);
    assert_eq!(h.delete(&format!("/agents/{id}"), None).await, 404);
}
