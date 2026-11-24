//! Skill system: external skills (http / command), enable flow, permissions,
//! credential handling, the MCP relay and its token boundary.

mod common;

use axum::Json;
use axum::extract::State;
use axum::routing::get;
use common::Harness;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Default)]
struct FakeApi {
    auth: Mutex<Vec<String>>,
    created: Mutex<Vec<Value>>,
}

async fn issues(State(st): State<Arc<FakeApi>>, h: axum::http::HeaderMap) -> Json<Value> {
    let a = h.get("authorization").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    st.auth.lock().unwrap().push(a.clone());
    // A careless API that echoes the credential back.
    Json(json!([{ "number": 1, "title": "First bug", "debug": a }]))
}

async fn create(State(st): State<Arc<FakeApi>>, Json(v): Json<Value>) -> Json<Value> {
    st.created.lock().unwrap().push(v.clone());
    Json(json!({ "number": 2, "title": v["title"] }))
}

fn skill(h: &Harness, id: &str, toml: &str, guide: &str) {
    let dir = h.paths.skills().join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("skill.toml"), toml).unwrap();
    std::fs::write(dir.join("SKILL.md"), guide).unwrap();
}

fn find<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["skills"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap_or(&Value::Null)
}

#[tokio::test]
async fn external_http_skill_lifecycle() {
    let api = Arc::new(FakeApi::default());
    let router = axum::Router::new()
        .route("/repos/me/app/issues", get(issues).post(create))
        .with_state(api.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });

    let h = Harness::new().await;
    skill(
        &h,
        "tracker",
        &format!(
            r#"id = "tracker"
name = "Tracker"
description = "Issue tracker"
[requires]
auth = true
[permissions]
network = ["127.0.0.1"]
[auth]
type = "token"
label = "API token"
[settings.owner]
label = "Owner"
default = "me"
[settings.api_base]
label = "API"
default = "{base}"
[[tools]]
name = "tracker_issues"
description = "List issues"
permission = "allow"
risk = "read"
handler = "http"
url = "{{{{settings.api_base}}}}/repos/{{{{owner}}}}/{{{{repo}}}}/issues"
input_schema = '''{{"type":"object","properties":{{"repo":{{"type":"string"}}}},"required":["repo"]}}'''
[[tools]]
name = "tracker_create"
description = "Create an issue"
handler = "http"
method = "POST"
url = "{{{{settings.api_base}}}}/repos/{{{{owner}}}}/{{{{repo}}}}/issues"
body = ["title"]
[[tools]]
name = "tracker_escape"
description = "Talks to an undeclared host"
permission = "allow"
handler = "http"
url = "https://example.com/{{{{repo}}}}"
"#
        ),
        "# Tracker\nAlways list before creating.",
    );

    // Discovered automatically, but off until the user enables it.
    let v = h.get("/skills").await;
    assert_eq!(find(&v, "tracker")["status"], "disabled");
    let tools = h.get("/tools").await;
    let t0 = tools["tools"].as_array().unwrap().iter().find(|t| t["name"] == "tracker_issues").unwrap().clone();
    assert_eq!(t0["available"], false);
    h.req(reqwest::Method::PATCH, "/skills/tracker", Some(json!({ "enabled": true }))).await;
    assert_eq!(find(&h.get("/skills").await, "tracker")["status"], "needs_setup");

    // Token login; the token never comes back from the API.
    h.post("/skills/tracker/login", json!({ "token": "tok-SECRET-123456" })).await;
    let v = h.get("/skills").await;
    assert_eq!(find(&v, "tracker")["status"], "ready");
    assert!(!v.to_string().contains("tok-SECRET"));

    // The model reads the guide, then calls the tool; the token is sent to the
    // API but scrubbed from what the model sees.
    let t = h.new_thread().await;
    let r = h.chat(&t, r#"calljson {"name":"skill_guide","arguments":{"skill":"tracker"}}"#, "cli").await;
    assert!(r["content"].as_str().unwrap().contains("Always list before creating"), "{r}");
    let r = h.chat(&t, r#"calljson {"name":"tracker_issues","arguments":{"repo":"app"}}"#, "cli").await;
    let c = r["content"].as_str().unwrap();
    assert!(c.contains("First bug") && c.contains("[redacted]") && !c.contains("tok-SECRET"), "{c}");
    assert_eq!(api.auth.lock().unwrap()[0], "Bearer tok-SECRET-123456");

    // Undeclared host is refused by the http handler.
    let r = h.chat(&t, r#"calljson {"name":"tracker_escape","arguments":{"repo":"x"}}"#, "cli").await;
    assert!(r["content"].as_str().unwrap().contains("not declared"), "{r}");

    // Default permission is ask: nothing happens until the user approves.
    let mut ev = h.events(Some(&t)).await;
    h.post(&format!("/threads/{t}/messages"), json!({ "content": r#"calljson {"name":"tracker_create","arguments":{"repo":"app","title":"New"}}"# })).await;
    let ask = ev.until(|v| v["type"] == "tool.approval_required").await;
    assert!(api.created.lock().unwrap().is_empty());
    h.post(&format!("/approvals/{}", ask["approval_id"].as_str().unwrap()), json!({ "approve": true })).await;
    ev.until_turn_done().await;
    assert_eq!(api.created.lock().unwrap()[0], json!({ "title": "New" }));

    // Disabling the skill removes its tools; the run log has the history.
    h.req(reqwest::Method::PATCH, "/skills/tracker", Some(json!({ "enabled": false }))).await;
    let r = h.chat(&t, r#"calljson {"name":"tracker_issues","arguments":{"repo":"app"}}"#, "cli").await;
    assert!(r["content"].as_str().unwrap().contains("not available"), "{r}");
    let runs = h.get("/tool-runs").await;
    let statuses: Vec<&str> = runs["runs"].as_array().unwrap().iter().map(|r| r["status"].as_str().unwrap()).collect();
    assert!(statuses.contains(&"ok") && statuses.contains(&"error") && statuses.contains(&"unavailable"), "{statuses:?}");
    assert!(!runs.to_string().contains("tok-SECRET"));

    // Deleting the thread deletes its log rows.
    h.delete(&format!("/threads/{t}"), None).await;
    assert_eq!(h.get("/tool-runs").await["runs"].as_array().unwrap().len(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn command_skill_runs_with_a_clean_environment() {
    let h = Harness::new().await;
    skill(
        &h,
        "echo",
        r#"id = "echo"
name = "Echo"
description = "Echo"
[[tools]]
name = "echo_env"
description = "Show input and environment"
permission = "allow"
handler = "command"
command = ["sh", "-c", "cat; echo; env | sort"]
"#,
        "",
    );
    h.req(reqwest::Method::PATCH, "/skills/echo", Some(json!({ "enabled": true }))).await;
    let t = h.new_thread().await;
    let r = h.chat(&t, r#"calljson {"name":"echo_env","arguments":{"x":1}}"#, "cli").await;
    let c = r["content"].as_str().unwrap();
    assert!(c.contains(r#""tool":"echo_env""#) && c.contains(r#""x":1"#), "{c}");
    // Only PATH/HOME/LANG/TMPDIR are passed: no API tokens or keys.
    assert!(c.contains("PATH=") && !c.contains("ALICE_") && !c.contains("sk-fake"), "{c}");
}

/// `alice mcp` relays to the server with a per-turn token; the gate still
/// asks the user, and the token opens nothing else.
#[tokio::test]
async fn mcp_relay_goes_through_the_gate() {
    let h = Harness::new().await;
    std::fs::write(h.paths.workspace().join("note.txt"), "hello").unwrap();
    let t = h.new_thread().await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let (token, _bridge) = h.app.open_bridge(alice::app::Bridge {
        thread_id: t.clone(),
        message_id: "none".into(),
        client: "cli".into(),
        cancel: cancel.clone(),
        calls: Default::default(),
    });

    // The turn token cannot use the normal API.
    let deny = |p: &'static str| {
        let (http, base, token) = (h.http.clone(), h.base.clone(), token.clone());
        async move {
            let r = http.get(format!("{base}/api{p}")).header("x-alice-turn", &token).bearer_auth(&token).send().await.unwrap();
            r.status().as_u16()
        }
    };
    assert_eq!(deny("/approvals").await, 401);
    assert_eq!(deny("/config").await, 401);
    assert_eq!(deny("/threads").await, 401);

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_alice"))
        .arg("mcp")
        .env("ALICE_MCP_TOKEN", &token)
        .env("ALICE_URL", &h.base)
        .env("ALICE_HOME", &h.paths.home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut rpc = async |id: i64, method: &str, params: Value| -> Value {
        stdin.write_all(format!("{}\n", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).as_bytes()).await.unwrap();
        loop {
            let line = out.next_line().await.unwrap().unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    };
    let init = rpc(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } })).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "alice");
    assert!(init["result"]["instructions"].as_str().unwrap().contains("files"));
    let list = rpc(2, "tools/list", json!({})).await;
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"fs_read") && names.contains(&"skill_guide"), "{names:?}");
    let ro = list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "fs_delete").unwrap();
    assert_eq!(ro["annotations"]["destructiveHint"], true);

    // Allowed tool runs.
    let r = rpc(3, "tools/call", json!({ "name": "fs_read", "arguments": { "path": "note.txt" } })).await;
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("hello"), "{r}");
    assert_eq!(r["result"]["isError"], false);

    // `ask` tool waits for the user; declining leaves the file.
    let mut ev = h.events(Some(&t)).await;
    let h2 = h.http.clone();
    let (base, api_token) = (h.base.clone(), h.token.clone());
    let approver = tokio::spawn(async move {
        let ask = ev.until(|v| v["type"] == "tool.approval_required").await;
        assert_eq!(ask["via"], "mcp");
        h2.post(format!("{base}/api/approvals/{}", ask["approval_id"].as_str().unwrap()))
            .bearer_auth(api_token)
            .json(&json!({ "approve": false }))
            .send()
            .await
            .unwrap();
    });
    let r = rpc(4, "tools/call", json!({ "name": "fs_delete", "arguments": { "path": "note.txt" } })).await;
    approver.await.unwrap();
    assert_eq!(r["result"]["isError"], true);
    assert!(h.paths.workspace().join("note.txt").exists());

    // Once the turn ends the token is dead.
    h.app.close_bridge(&token);
    let r = rpc(5, "tools/call", json!({ "name": "fs_read", "arguments": { "path": "note.txt" } })).await;
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("expired"), "{r}");

    let runs = h.get("/tool-runs").await.to_string();
    assert!(runs.contains("\"source\":\"mcp\"") && runs.contains("declined"), "{runs}");
}

#[test]
fn text_protocol_and_codex_tool_scan() {
    use alice::providers::cli::{codex_tool_names, parse_text_calls};
    let (text, calls) = parse_text_calls("Sure.\n```alice-tool\n{\"name\":\"calendar_create\",\"arguments\":{\"summary\":\"x\"}}\n```\n");
    assert_eq!(text, "Sure.");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "calendar_create");
    assert_eq!(calls[0].arguments["summary"], "x");
    let (_, none) = parse_text_calls("```alice-tool\nnot json\n```");
    assert!(none.is_empty());

    let prompt = json!({ "tools": [
        { "type": "custom", "name": "exec", "description": "Run JS\n### `apply_patch`\n…\n### `exec_command`\n…" },
        { "type": "function", "name": "spawn_agent", "parameters": {} },
    ]});
    let names = codex_tool_names(&prompt);
    assert!(names.contains(&"exec_command".to_string()) && names.contains(&"apply_patch".to_string()), "{names:?}");
}

/// Needs the real Codex CLI: `cargo test --test skills -- --ignored`.
#[tokio::test]
#[ignore]
async fn real_codex_passes_the_safety_check() {
    let dir = tempfile::tempdir().unwrap();
    alice::providers::cli::set_codex_home(dir.path().join("codex"));
    let r = alice::providers::cli::codex_safe("codex").await;
    let cfg = std::fs::read_to_string(dir.path().join("codex/config.toml")).unwrap();
    assert!(cfg.contains("shell_tool = false") && cfg.contains("sandbox_mode = \"read-only\""));
    assert!(r.is_ok(), "{r:?}");
}
