//! Test harness: a scripted OpenAI-compatible LLM server and an Alice
//! server on random local ports. The fake LLM is test infrastructure only.

#![allow(dead_code)]

use alice::config::{Config, Paths, ProviderConfig, ProviderKind, Secrets};
use alice::providers::SseLines;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
pub struct FakeLlm {
    pub requests: Mutex<Vec<Value>>,
}

fn sse(chunks: Vec<Value>, delay_ms: u64) -> axum::response::Response {
    let stream = futures_util::stream::unfold((chunks.into_iter(), true), move |(mut it, first)| async move {
        if !first && delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        it.next().map(|c| (Ok::<_, std::convert::Infallible>(format!("data: {c}\n\n")), (it, false)))
    });
    let done = futures_util::stream::once(async { Ok::<_, std::convert::Infallible>("data: [DONE]\n\n".to_string()) });
    let body = axum::body::Body::from_stream(futures_util::StreamExt::chain(stream, done));
    ([("content-type", "text/event-stream")], body).into_response()
}

fn text_chunks(text: &str) -> Vec<Value> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(8)
        .map(|c| json!({ "choices": [{ "index": 0, "delta": { "content": c.iter().collect::<String>() } }] }))
        .chain(std::iter::once(json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] })))
        .collect()
}

/// Scripted behaviour:
/// - memory extraction requests: "I live in X" -> home_city; "forget where I live" -> forget;
///   "slowmem" delays the reply by 1.5s.
/// - chat: replies `MEM[<memories block lines>] N=<message count> MODEL=<model> ECHO=<last user text>`;
///   "slow" streams slowly; "call fs_list"/"call fs_delete" request a tool call once.
async fn chat(State(st): State<Arc<FakeLlm>>, Json(req): Json<Value>) -> axum::response::Response {
    st.requests.lock().unwrap().push(req.clone());
    let msgs = req["messages"].as_array().cloned().unwrap_or_default();
    let system = msgs.iter().find(|m| m["role"] == "system").and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
    let last_user = msgs.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
    let model = req["model"].as_str().unwrap_or("").to_string();

    if system.contains("You maintain long-term memory") {
        let user_msg = last_user.split("USER MESSAGE:\n").nth(1).unwrap_or("").to_string();
        if user_msg.contains("slowmem") {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        let mut items = vec![];
        if let Some(rest) = user_msg.split("I live in ").nth(1) {
            let city: String = rest.chars().take_while(|c| c.is_alphabetic()).collect();
            items.push(json!({ "key": "home_city", "content": format!("The user lives in {city}.") }));
        }
        if user_msg.contains("forget where I live") {
            items.push(json!({ "key": "home_city", "forget": true }));
        }
        if let Some(rest) = user_msg.split("My favorite color is ").nth(1) {
            let c: String = rest.chars().take_while(|c| c.is_alphabetic()).collect();
            items.push(json!({ "key": "favorite_color", "content": format!("The user's favorite color is {c}.") }));
        }
        return sse(text_chunks(&json!({ "memories": items }).to_string()), 0);
    }

    let last_user_idx = msgs.iter().rposition(|m| m["role"] == "user").unwrap_or(0);
    let has_tool_result = msgs[last_user_idx..].iter().any(|m| m["role"] == "tool");
    for tool in ["fs_list", "fs_delete"] {
        if last_user.contains(&format!("call {tool}")) && !has_tool_result {
            let args = if tool == "fs_list" { json!({ "path": "." }) } else { json!({ "path": "victim.txt" }) };
            return sse(
                vec![
                    json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_1", "type": "function",
                        "function": { "name": tool, "arguments": "" } }] } }] }),
                    json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": args.to_string() } }] } }] }),
                    json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }),
                ],
                0,
            );
        }
    }
    // "calljson {"name": ..., "arguments": {...}}" requests any tool once.
    if let (Some(spec), false) = (last_user.strip_prefix("calljson "), has_tool_result) {
        let v: Value = serde_json::from_str(spec).unwrap();
        return sse(
            vec![
                json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_j", "type": "function",
                    "function": { "name": v["name"], "arguments": v["arguments"].to_string() } }] } }] }),
                json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }),
            ],
            0,
        );
    }
    let tool_text = msgs[last_user_idx..].iter().filter(|m| m["role"] == "tool").filter_map(|m| m["content"].as_str()).collect::<Vec<_>>().join("|");
    let mem: Vec<&str> = system
        .rsplit_once("<memories>\n")
        .and_then(|(_, s)| s.split("</memories>").next())
        .map(|s| s.lines().filter(|l| l.starts_with("- ")).collect())
        .unwrap_or_default();
    let reply = format!(
        "MEM[{}] N={} MODEL={} TOOL[{}] ECHO={}",
        mem.join(";"),
        msgs.iter().filter(|m| m["role"] != "system").count(),
        model,
        tool_text,
        last_user
    );
    let delay = if last_user.contains("slow") { 150 } else { 0 };
    sse(text_chunks(&reply), delay)
}

pub struct Harness {
    pub dir: tempfile::TempDir,
    pub paths: Paths,
    pub base: String,
    pub token: String,
    pub http: reqwest::Client,
    pub llm: Arc<FakeLlm>,
    pub llm_url: String,
    pub app: alice::app::AppRef,
    server: tokio::task::JoinHandle<()>,
}

pub async fn fake_llm() -> (Arc<FakeLlm>, String) {
    let st = Arc::new(FakeLlm::default());
    let router = Router::new().route("/v1/chat/completions", post(chat)).with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (st, url)
}

pub fn write_config(paths: &Paths, llm_url: &str) {
    paths.ensure().unwrap();
    let mut cfg = Config::default();
    cfg.providers.push(ProviderConfig {
        id: "fake".into(),
        kind: ProviderKind::OpenaiCompatible,
        base_url: llm_url.into(),
        models: vec!["m1".into(), "m2".into()],
        context_tokens: 0,
    });
    cfg.agent.default_model = "fake/m1".into();
    cfg.memory.min_chars = 1;
    cfg.save(paths).unwrap();
    let mut s = Secrets::default();
    s.set("server.token", "test-token-0123456789");
    s.set("provider.fake.api_key", "sk-fake-key");
    s.save(paths).unwrap();
}

impl Harness {
    pub async fn new() -> Harness {
        Self::with(|_, _| {}).await
    }

    /// Start with a customised config / secrets.
    pub async fn with(f: impl FnOnce(&mut Config, &mut Secrets)) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path().join("alice"));
        let (llm, llm_url) = fake_llm().await;
        write_config(&paths, &llm_url);
        let mut cfg = Config::load(&paths).unwrap();
        let mut sec = Secrets::load(&paths).unwrap();
        f(&mut cfg, &mut sec);
        cfg.save(&paths).unwrap();
        sec.save(&paths).unwrap();
        Self::start(dir, paths, llm, llm_url).await
    }

    pub async fn start(dir: tempfile::TempDir, paths: Paths, llm: Arc<FakeLlm>, llm_url: String) -> Harness {
        alice::util::set_log_quiet(true);
        let app = alice::build_app(paths.clone()).unwrap();
        let router = alice::server::router(app.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        // With the client address, like the real server (per-address token guard).
        let svc = router.into_make_service_with_connect_info::<std::net::SocketAddr>();
        let server = tokio::spawn(async move { axum::serve(listener, svc).await.unwrap() });
        Harness {
            dir,
            paths,
            base,
            token: "test-token-0123456789".into(),
            http: reqwest::Client::new(),
            llm,
            llm_url,
            app,
            server,
        }
    }

    /// Stop this server and start a fresh process-equivalent on the same data dir.
    pub async fn restart(self) -> Harness {
        self.server.abort();
        let Harness { dir, paths, llm, llm_url, app, .. } = self;
        drop(app);
        Self::start(dir, paths, llm, llm_url).await
    }

    pub fn url(&self, p: &str) -> String {
        format!("{}/api{}", self.base, p)
    }

    pub async fn req(&self, method: reqwest::Method, p: &str, body: Option<Value>) -> (u16, Value) {
        let mut rb = self.http.request(method, self.url(p)).bearer_auth(&self.token);
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let r = rb.send().await.unwrap();
        let code = r.status().as_u16();
        let text = r.text().await.unwrap();
        (code, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn get(&self, p: &str) -> Value {
        let (c, v) = self.req(reqwest::Method::GET, p, None).await;
        assert!(c < 300, "GET {p} -> {c} {v}");
        v
    }
    pub async fn post(&self, p: &str, b: Value) -> Value {
        let (c, v) = self.req(reqwest::Method::POST, p, Some(b)).await;
        assert!(c < 300, "POST {p} -> {c} {v}");
        v
    }
    pub async fn delete(&self, p: &str, b: Option<Value>) -> u16 {
        self.req(reqwest::Method::DELETE, p, b).await.0
    }

    pub async fn new_thread(&self) -> String {
        self.post("/threads", json!({})).await["thread"]["id"].as_str().unwrap().to_string()
    }

    pub async fn events(&self, thread: Option<&str>) -> Events {
        let mut rb = self.http.get(self.url("/events")).bearer_auth(&self.token);
        if let Some(t) = thread {
            rb = rb.query(&[("thread", t)]);
        }
        let r = rb.send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        let mut e = Events { lines: SseLines::new(r) };
        let hello = e.next().await;
        assert_eq!(hello["type"], "hello");
        e
    }

    /// Post a message and wait until the turn's final assistant message completes.
    pub async fn chat(&self, thread: &str, text: &str, client: &str) -> Value {
        let mut ev = self.events(Some(thread)).await;
        self.post(&format!("/threads/{thread}/messages"), json!({ "content": text, "client": client })).await;
        ev.until_turn_done().await
    }

    /// Wait (bounded) until the memory list satisfies `pred`.
    pub async fn wait_memories(&self, pred: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        for _ in 0..100 {
            let v = self.get("/memories?limit=1000").await;
            let list = v["memories"].as_array().cloned().unwrap_or_default();
            if pred(&list) {
                return list;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        panic!("memory condition not reached");
    }

    pub fn extraction_requests(&self) -> usize {
        self.llm
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["messages"][0]["content"].as_str().unwrap_or("").contains("You maintain long-term memory"))
            .count()
    }
}

pub struct Events {
    lines: SseLines,
}

impl Events {
    pub async fn next(&mut self) -> Value {
        let d = tokio::time::timeout(Duration::from_secs(10), self.lines.next_data())
            .await
            .expect("timed out waiting for event")
            .unwrap()
            .expect("stream closed");
        serde_json::from_str(&d).unwrap()
    }

    pub async fn until(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        loop {
            let v = self.next().await;
            if pred(&v) {
                return v;
            }
        }
    }

    pub async fn until_turn_done(&mut self) -> Value {
        self.until(|v| {
            v["type"] == "message.completed"
                && v["message"]["role"] == "assistant"
                && (v["message"]["status"] != "complete"
                    || v["message"]["tool_calls"].as_array().is_none_or(|a| a.is_empty()))
        })
        .await["message"]
            .clone()
    }
}
