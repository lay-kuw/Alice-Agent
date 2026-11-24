//! End-to-end tests for the requirements in docs/SPEC.md (§20 of the brief).

mod common;

use common::Harness;
use serde_json::json;
use std::time::Duration;

fn content(m: &serde_json::Value) -> String {
    m["content"].as_str().unwrap_or("").to_string()
}

#[tokio::test]
async fn multiple_clients_share_one_thread() {
    let h = Harness::new().await;
    let t = h.new_thread().await;
    // A second client watches the global stream.
    let mut watcher = h.events(None).await;

    let a = h.chat(&t, "hello from cli", "cli").await;
    assert!(content(&a).contains("ECHO=hello from cli"));
    let b = h.chat(&t, "hello from phone", "ios").await;
    // The second turn sees the first turn's history (2 user + 1 assistant before it).
    assert!(content(&b).contains("N=3"), "{}", content(&b));

    let msgs = h.get(&format!("/threads/{t}/messages")).await;
    let list = msgs["messages"].as_array().unwrap();
    let clients: Vec<&str> = list.iter().filter(|m| m["role"] == "user").map(|m| m["client"].as_str().unwrap()).collect();
    assert_eq!(clients, vec!["cli", "ios"]);

    // The watcher saw deltas and completions for the same thread in real time.
    let ev = watcher.until(|v| v["type"] == "message.delta").await;
    assert_eq!(ev["thread_id"], t);
    // The thread got an automatic title, broadcast to all clients.
    let th = h.get(&format!("/threads/{t}")).await;
    assert_eq!(th["thread"]["title"], "hello from cli");

    // Concurrent generation in the same thread is rejected.
    let mut ev = h.events(Some(&t)).await;
    h.post(&format!("/threads/{t}/messages"), json!({ "content": "slow one", "client": "web" })).await;
    let (code, _) = h.req(reqwest::Method::POST, &format!("/threads/{t}/messages"), Some(json!({ "content": "again" }))).await;
    assert_eq!(code, 409);
    ev.until_turn_done().await;
}

#[tokio::test]
async fn memory_is_shared_across_threads() {
    let h = Harness::new().await;
    let a = h.new_thread().await;
    h.chat(&a, "By the way, I live in Osaka.", "cli").await;
    let mems = h.wait_memories(|l| l.len() == 1).await;
    assert_eq!(mems[0]["key"], "home_city");

    let b = h.new_thread().await;
    let reply = h.chat(&b, "Where do I live? Osaka or Tokyo?", "web").await;
    assert!(content(&reply).contains("The user lives in Osaka."), "{}", content(&reply));

    // Provenance: the memory points at thread A's user message.
    let id = mems[0]["id"].as_i64().unwrap();
    let detail = h.get(&format!("/memories/{id}")).await;
    let sources = detail["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["thread_id"], a);

    // Only relevant memories are injected.
    h.chat(&a, "My favorite color is green", "cli").await;
    h.wait_memories(|l| l.len() == 2).await;
    let r = h.chat(&b, "what is my favorite color", "web").await;
    assert!(content(&r).contains("green") && !content(&r).contains("Osaka"), "{}", content(&r));
}

#[tokio::test]
async fn deleting_a_thread_deletes_its_memories() {
    let h = Harness::new().await;
    let a = h.new_thread().await;
    h.chat(&a, "I live in Osaka", "cli").await;
    h.wait_memories(|l| l.len() == 1).await;
    assert_eq!(h.delete(&format!("/threads/{a}"), None).await, 204);

    assert!(h.get("/memories").await["memories"].as_array().unwrap().is_empty());
    assert!(h.get("/memories?q=Osaka").await["memories"].as_array().unwrap().is_empty());
    let (code, _) = h.req(reqwest::Method::GET, &format!("/threads/{a}/messages"), None).await;
    assert_eq!(code, 404);

    let b = h.new_thread().await;
    let r = h.chat(&b, "Where do I live? Osaka?", "web").await;
    assert!(content(&r).starts_with("MEM[]"), "{}", content(&r));

    // Physically gone from the DB file (secure_delete + WAL checkpoint).
    let db = std::fs::read(h.paths.db()).unwrap();
    assert!(!db.windows(5).any(|w| w == b"Osaka" ), "deleted text still present in alice.db");
}

#[tokio::test]
async fn derived_information_does_not_resurrect() {
    let h = Harness::new().await;
    let a = h.new_thread().await;
    h.chat(&a, "I live in Osaka", "cli").await;
    h.wait_memories(|l| l.len() == 1).await;

    // Thread B gets an answer that repeats the memory.
    let b = h.new_thread().await;
    let r = h.chat(&b, "Where do I live, Osaka?", "web").await;
    assert!(content(&r).contains("Osaka"));
    let before = h.extraction_requests();
    h.chat(&b, "thanks, that is right", "web").await;
    // Wait for B's extraction calls to finish.
    for _ in 0..100 {
        if h.extraction_requests() > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Extraction never saw assistant text or memory contents.
    for r in h.llm.requests.lock().unwrap().iter() {
        let sys = r["messages"][0]["content"].as_str().unwrap_or("");
        if sys.contains("You maintain long-term memory") {
            let input = r["messages"][1]["content"].as_str().unwrap_or("");
            assert!(!input.contains("MEM["), "assistant output leaked into extraction");
            assert!(!input.contains("The user lives in"), "memory content leaked into extraction");
        }
    }

    h.delete(&format!("/threads/{a}"), None).await;
    let mems = h.get("/memories").await;
    assert!(mems["memories"].as_array().unwrap().is_empty(), "{mems}");
    // Continuing in B does not bring it back either.
    h.chat(&b, "ok, see you", "web").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(h.get("/memories").await["memories"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn independent_sources_survive_and_content_is_rebuilt() {
    let h = Harness::new().await;
    let a = h.new_thread().await;
    h.chat(&a, "I live in Osaka", "cli").await;
    h.wait_memories(|l| l.len() == 1).await;

    // The user independently says the same thing elsewhere: second source.
    let c = h.new_thread().await;
    h.chat(&c, "I live in Osaka", "web").await;
    let m = h.wait_memories(|l| l.len() == 1 && l[0]["source_count"] == 2).await;
    assert_eq!(m[0]["source_count"], 2);
    h.delete(&format!("/threads/{a}"), None).await;
    let m = h.get("/memories").await["memories"].as_array().unwrap().clone();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0]["source_count"], 1);

    // An update from another thread, then deleting that thread, reverts the content.
    let d = h.new_thread().await;
    h.chat(&d, "I live in Kyoto now", "web").await;
    h.wait_memories(|l| l.len() == 1 && l[0]["content"].as_str().unwrap().contains("Kyoto")).await;
    h.delete(&format!("/threads/{d}"), None).await;
    let m = h.get("/memories").await["memories"].as_array().unwrap().clone();
    assert!(m[0]["content"].as_str().unwrap().contains("Osaka"), "{m:?}");
    assert!(h.get("/memories?q=Kyoto").await["memories"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn delete_all_resets_memory_but_keeps_settings() {
    let h = Harness::new().await;
    for city in ["Osaka", "Nagoya"] {
        let t = h.new_thread().await;
        h.chat(&t, &format!("I live in {city}"), "cli").await;
    }
    h.wait_memories(|l| l.len() == 1 && l[0]["source_count"] == 2).await;
    let (code, _) = h.req(reqwest::Method::DELETE, "/threads", Some(json!({ "confirm": "nope" }))).await;
    assert_eq!(code, 400);
    assert_eq!(h.delete("/threads", Some(json!({ "confirm": "delete-all" }))).await, 204);

    let st = h.get("/status").await;
    assert_eq!(st["counts"]["threads"], 0);
    assert_eq!(st["counts"]["messages"], 0);
    assert_eq!(st["counts"]["memories"], 0);
    let ms = h.get("/memories/status").await;
    assert_eq!(ms["status"]["sources"], 0);

    // Settings and credentials are intact.
    let models = h.get("/models").await;
    assert_eq!(models["default"], "fake/m1");
    assert_eq!(models["providers"][0]["has_key"], true);
    let secrets = std::fs::read_to_string(h.paths.secrets()).unwrap();
    assert!(secrets.contains("sk-fake-key") && secrets.contains("server.token"));

    let db = std::fs::read(h.paths.db()).unwrap();
    assert!(!db.windows(5).any(|w| w == b"Osaka"));
    let wal = std::fs::metadata(format!("{}-wal", h.paths.db().display())).map(|m| m.len()).unwrap_or(0);
    assert_eq!(wal, 0, "WAL should be truncated after reset");

    // Alice still works as new.
    let t = h.new_thread().await;
    let r = h.chat(&t, "Where do I live?", "cli").await;
    assert!(content(&r).starts_with("MEM[]"));
}

#[tokio::test]
async fn deleting_during_generation_does_not_resurrect() {
    let h = Harness::new().await;
    let t = h.new_thread().await;
    let mut ev = h.events(Some(&t)).await;
    h.post(&format!("/threads/{t}/messages"), json!({ "content": "slow: I live in Sapporo", "client": "cli" })).await;
    ev.until(|v| v["type"] == "message.delta").await;
    assert_eq!(h.delete(&format!("/threads/{t}"), None).await, 204);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let st = h.get("/status").await;
    assert_eq!(st["counts"]["threads"], 0);
    assert_eq!(st["counts"]["messages"], 0);
    assert!(st["generating"].as_array().unwrap().is_empty());
    assert!(h.get("/memories").await["memories"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn deleting_during_memory_extraction_does_not_resurrect() {
    let h = Harness::new().await;
    let t = h.new_thread().await;
    // Reply is fast; extraction for this message takes 1.5s.
    h.chat(&t, "slowmem I live in Sendai", "cli").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.delete(&format!("/threads/{t}"), None).await, 204);
    tokio::time::sleep(Duration::from_millis(1800)).await;
    assert_eq!(h.extraction_requests(), 1);
    assert!(h.get("/memories").await["memories"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn stop_generation_keeps_partial_answer() {
    let h = Harness::new().await;
    let t = h.new_thread().await;
    let mut ev = h.events(Some(&t)).await;
    h.post(&format!("/threads/{t}/messages"), json!({ "content": "slow please write a lot", "client": "web" })).await;
    ev.until(|v| v["type"] == "message.delta").await;
    let r = h.post(&format!("/threads/{t}/stop"), json!({})).await;
    assert_eq!(r["stopped"], true);
    ev.until(|v| v["type"] == "generation.cancelled").await;
    let m = ev.until_turn_done().await;
    assert_eq!(m["status"], "cancelled");
    assert!(!content(&m).is_empty());
    // The thread is usable again.
    let r = h.chat(&t, "next", "web").await;
    assert_eq!(r["status"], "complete");
}

#[tokio::test]
async fn switching_models_continues_the_conversation() {
    let h = Harness::new().await;
    let t = h.new_thread().await;
    let r1 = h.chat(&t, "first", "cli").await;
    assert!(content(&r1).contains("MODEL=m1"));
    let (code, _) = h.req(reqwest::Method::PATCH, &format!("/threads/{t}"), Some(json!({ "model": "fake/m2" }))).await;
    assert_eq!(code, 200);
    let r2 = h.chat(&t, "second", "web").await;
    assert!(content(&r2).contains("MODEL=m2") && content(&r2).contains("N=3"), "{}", content(&r2));
    let (code, _) = h.req(reqwest::Method::PATCH, &format!("/threads/{t}"), Some(json!({ "model": "nope/x" }))).await;
    assert_eq!(code, 400);
}

#[tokio::test]
async fn unauthenticated_access_is_rejected() {
    let h = Harness::new().await;
    let c = reqwest::Client::new();
    assert_eq!(c.get(h.url("/health")).send().await.unwrap().status(), 200);
    for p in ["/threads", "/events", "/memories", "/config", "/status"] {
        assert_eq!(c.get(h.url(p)).send().await.unwrap().status(), 401, "{p}");
        assert_eq!(c.get(h.url(p)).bearer_auth("wrong").send().await.unwrap().status(), 401, "{p}");
    }
    assert_eq!(c.delete(h.url("/threads")).json(&json!({"confirm":"delete-all"})).send().await.unwrap().status(), 401);

    // Login issues a cookie; cookie auth needs the CSRF header for writes.
    let bad = c.post(h.url("/login")).json(&json!({ "token": "wrong" })).send().await.unwrap();
    assert_eq!(bad.status(), 401);
    let ok = c.post(h.url("/login")).json(&json!({ "token": h.token })).send().await.unwrap();
    let cookie = ok.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();
    assert!(ok.headers()["set-cookie"].to_str().unwrap().contains("HttpOnly"));
    let r = c.get(h.url("/threads")).header("cookie", &cookie).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let r = c.post(h.url("/threads")).header("cookie", &cookie).json(&json!({})).send().await.unwrap();
    assert_eq!(r.status(), 403);
    let r = c.post(h.url("/threads")).header("cookie", &cookie).header("x-alice-csrf", "1").json(&json!({})).send().await.unwrap();
    assert_eq!(r.status(), 201);

    // Secrets are never returned by the API.
    let cfg = h.get("/config").await.to_string();
    assert!(!cfg.contains("sk-fake-key") && !cfg.contains(&h.token));
    let page = c.get(format!("{}/", h.base)).send().await.unwrap();
    assert!(page.headers()["content-security-policy"].to_str().unwrap().contains("default-src 'self'"));
}

#[tokio::test]
async fn tool_permissions_are_enforced() {
    let h = Harness::new().await;
    let ws = h.paths.workspace();
    std::fs::write(ws.join("victim.txt"), "keep me").unwrap();
    let t = h.new_thread().await;

    // Allowed by default: fs_list runs.
    let r = h.chat(&t, "call fs_list", "cli").await;
    assert!(content(&r).contains("victim.txt"), "{}", content(&r));

    // Destructive by default: fs_delete asks; rejecting keeps the file.
    let mut ev = h.events(Some(&t)).await;
    h.post(&format!("/threads/{t}/messages"), json!({ "content": "call fs_delete", "client": "web" })).await;
    let ask = ev.until(|v| v["type"] == "tool.approval_required").await;
    assert_eq!(ask["tool"], "fs_delete");
    assert_eq!(ask["client"], "web");
    h.post(&format!("/approvals/{}", ask["approval_id"].as_str().unwrap()), json!({ "approve": false })).await;
    let m = ev.until_turn_done().await;
    assert!(content(&m).contains("declined"), "{}", content(&m));
    assert!(ws.join("victim.txt").exists());

    // Disabled: the tool is not offered and calls are refused.
    let (code, _) = h.req(reqwest::Method::PATCH, "/tools", Some(json!({ "fs_list": "disabled" }))).await;
    assert_eq!(code, 200);
    let t2 = h.new_thread().await;
    let r = h.chat(&t2, "call fs_list", "cli").await;
    assert!(content(&r).contains("disabled by the user"), "{}", content(&r));
    let last = h.llm.requests.lock().unwrap().last().cloned().unwrap();
    let offered: Vec<String> = last["tools"].as_array().map(|a| a.iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect()).unwrap_or_default();
    assert!(!offered.contains(&"fs_list".to_string()));
    // No tool lets the model change permissions.
    assert!(offered.iter().all(|n| n.starts_with("fs_")));

    // Approving runs it.
    let (code, _) = h.req(reqwest::Method::PATCH, "/tools", Some(json!({ "fs_list": "default" }))).await;
    assert_eq!(code, 200);
    let t3 = h.new_thread().await;
    let mut ev = h.events(Some(&t3)).await;
    h.post(&format!("/threads/{t3}/messages"), json!({ "content": "call fs_delete", "client": "cli" })).await;
    let ask = ev.until(|v| v["type"] == "tool.approval_required").await;
    h.post(&format!("/approvals/{}", ask["approval_id"].as_str().unwrap()), json!({ "approve": true })).await;
    ev.until_turn_done().await;
    assert!(!ws.join("victim.txt").exists());
}

#[tokio::test]
async fn conversation_survives_restart() {
    let h = Harness::new().await;
    let t = h.new_thread().await;
    h.chat(&t, "before restart", "cli").await;
    // Simulate a crash mid-generation.
    h.app.db.lock().execute("UPDATE messages SET status = 'streaming' WHERE role = 'assistant'", []).unwrap();

    let h = h.restart().await;
    let msgs = h.get(&format!("/threads/{t}/messages")).await;
    let list = msgs["messages"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1]["status"], "cancelled");
    let r = h.chat(&t, "after restart", "web").await;
    assert_eq!(r["status"], "complete");
    assert!(content(&r).contains("ECHO=after restart"));
}
