//! Codex through a long-lived `codex app-server` (JSON-RPC over stdio).
//!
//! `codex exec` starts a new process for every turn, which costs seconds
//! before the first token. Alice keeps one app-server running (with Alice's
//! own CODEX_HOME) and starts a fresh ephemeral Codex thread per Alice turn,
//! so each turn still gets the full prompt Alice builds and nothing carries
//! over between turns. Replies stream as they are generated.
//!
//! Alice's tools are given to Codex as *dynamic tools*: Codex asks Alice to
//! run them over the same connection (`item/tool/call`), and Alice answers
//! after its Tool Gate (permissions, approval, logging). No MCP subprocess
//! and no token are involved, so there is nothing for the model to bypass.

use super::{Completion, ProviderError, Result, ToolRunner, ToolSpec};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, oneshot};

/// Recycle the process after this many turns (bounded memory).
const MAX_TURNS: usize = 200;

struct Proc {
    stdin: tokio::sync::Mutex<ChildStdin>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, oneshot::Sender<Value>>>,
    /// Codex thread id -> its notifications and server requests.
    subs: Mutex<HashMap<String, mpsc::UnboundedSender<Value>>>,
    alive: AtomicBool,
    turns: AtomicUsize,
    child: Mutex<Option<Child>>,
}

impl Proc {
    async fn write(&self, v: &Value) -> std::result::Result<(), String> {
        let mut w = self.stdin.lock().await;
        w.write_all(format!("{v}\n").as_bytes()).await.map_err(|e| e.to_string())?;
        w.flush().await.map_err(|e| e.to_string())
    }

    async fn request(&self, method: &str, params: Value) -> std::result::Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        self.write(&json!({ "id": id, "method": method, "params": params })).await?;
        let v = tokio::time::timeout(Duration::from_secs(60), rx)
            .await
            .map_err(|_| format!("codex did not answer {method}"))?
            .map_err(|_| "codex app-server exited".to_string())?;
        match v.get("error") {
            Some(e) => Err(e["message"].as_str().unwrap_or("codex error").to_string()),
            None => Ok(v["result"].clone()),
        }
    }

    async fn respond(&self, id: &Value, result: Value) {
        let _ = self.write(&json!({ "id": id, "result": result })).await;
    }

    fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Some(mut c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.start_kill();
        }
    }
}

/// One app-server per Codex executable.
static PROCS: std::sync::LazyLock<tokio::sync::Mutex<HashMap<String, Arc<Proc>>>> = std::sync::LazyLock::new(Default::default);

/// The running app-server for `exe`, started (and initialized) on demand.
async fn get(exe: &str) -> std::result::Result<Arc<Proc>, String> {
    let mut procs = PROCS.lock().await;
    if let Some(p) = procs.get(exe)
        && p.alive.load(Ordering::SeqCst)
        && p.turns.load(Ordering::SeqCst) < MAX_TURNS
    {
        return Ok(p.clone());
    }
    // A retired process lets running turns finish, then stops.
    if let Some(old) = procs.remove(exe) {
        tokio::spawn(async move {
            if old.alive.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_secs(600)).await;
            }
            old.kill();
        });
    }
    let mut child = super::cli::codex_cmd(exe)
        .arg("app-server")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("cannot start '{exe}': {e}"))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let p = Arc::new(Proc {
        stdin: tokio::sync::Mutex::new(stdin),
        next_id: AtomicI64::new(1),
        pending: Mutex::new(HashMap::new()),
        subs: Mutex::new(HashMap::new()),
        alive: AtomicBool::new(true),
        turns: AtomicUsize::new(0),
        child: Mutex::new(Some(child)),
    });
    tokio::spawn(read_loop(p.clone(), stdout));
    let init = json!({ "clientInfo": { "name": "alice", "version": env!("CARGO_PKG_VERSION") }, "capabilities": { "experimentalApi": true } });
    if let Err(e) = p.request("initialize", init).await {
        p.kill();
        return Err(e);
    }
    p.write(&json!({ "method": "initialized" })).await?;
    crate::info!("codex app-server started");
    procs.insert(exe.to_string(), p.clone());
    Ok(p)
}

/// Start the app-server ahead of the first turn (no tokens are spent).
pub fn warm_up(exe: String) {
    tokio::spawn(async move {
        if let Err(e) = get(&exe).await {
            crate::warn!("codex app-server did not start: {e}");
        }
    });
}

/// Stop the app-server (e.g. after a login change); the next turn restarts it.
pub async fn reset() {
    for (_, p) in PROCS.lock().await.drain() {
        p.kill();
    }
}

async fn read_loop(p: Arc<Proc>, stdout: tokio::process::ChildStdout) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let method = v.get("method").and_then(|m| m.as_str());
        match (method, v.get("id")) {
            // Response to one of our requests.
            (None, Some(id)) => {
                if let Some(tx) = id.as_i64().and_then(|i| p.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&i)) {
                    let _ = tx.send(v);
                }
            }
            // Notification or server request: route by Codex thread.
            (Some(_), id) => {
                let thread = v["params"]["threadId"].as_str().or(v["params"]["thread"]["id"].as_str()).map(str::to_string);
                let sub = thread.and_then(|t| p.subs.lock().unwrap_or_else(|e| e.into_inner()).get(&t).cloned());
                match (sub, id) {
                    (Some(tx), _) => {
                        let _ = tx.send(v);
                    }
                    // A request nobody can answer (auth refresh, approvals): decline it.
                    (None, Some(id)) => {
                        let _ = p.write(&json!({ "id": id, "error": { "code": -32601, "message": "not supported by Alice" } })).await;
                    }
                    (None, None) => {}
                }
            }
            _ => {}
        }
    }
    p.alive.store(false, Ordering::SeqCst);
    // Wake everyone waiting on this process.
    p.pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
    p.subs.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Interrupts the turn and drops the subscription when the caller goes away
/// (stop button, client disconnect) or the turn ends.
struct TurnGuard {
    p: Arc<Proc>,
    thread: String,
    turn: Option<String>,
    done: bool,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.p.subs.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.thread);
        let (p, thread, turn, done) = (self.p.clone(), self.thread.clone(), self.turn.clone(), self.done);
        tokio::spawn(async move {
            if !done && let Some(turn) = turn {
                let _ = p.request("turn/interrupt", json!({ "threadId": thread, "turnId": turn })).await;
            }
            let _ = p.request("thread/unsubscribe", json!({ "threadId": thread })).await;
        });
    }
}

/// What one turn needs.
pub struct Turn<'a> {
    pub exe: &'a str,
    pub prompt: &'a str,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub cwd: &'a std::path::Path,
    /// Tools offered natively; None = no tools (prompt-only modes).
    pub tools: Option<(&'a [ToolSpec], &'a ToolRunner)>,
}

/// Run one turn. Errors before the turn starts are `Err(Unavailable)` so the
/// caller can fall back to `codex exec`.
pub async fn run(t: Turn<'_>, on_delta: super::OnDelta<'_>) -> std::result::Result<Result<Completion>, String> {
    let p = get(t.exe).await?;
    let mut params = json!({ "ephemeral": true, "cwd": t.cwd, "sandbox": "read-only", "approvalPolicy": "never" });
    if let Some((specs, _)) = t.tools {
        params["dynamicTools"] = Value::Array(
            specs.iter().map(|s| json!({ "type": "function", "name": s.name, "description": s.description, "inputSchema": s.parameters })).collect(),
        );
    }
    let started = p.request("thread/start", params).await?;
    let thread = started["thread"]["id"].as_str().ok_or("codex returned no thread id")?.to_string();
    let (tx, mut rx) = mpsc::unbounded_channel();
    p.subs.lock().unwrap_or_else(|e| e.into_inner()).insert(thread.clone(), tx);
    let mut guard = TurnGuard { p: p.clone(), thread: thread.clone(), turn: None, done: false };
    p.turns.fetch_add(1, Ordering::SeqCst);

    let mut turn = json!({ "threadId": thread, "input": [{ "type": "text", "text": t.prompt }] });
    if !t.model.is_empty() && t.model != "default" {
        turn["model"] = json!(t.model);
    }
    if let Some(e) = t.effort {
        turn["effort"] = json!(e);
    }
    let r = p.request("turn/start", turn).await?;
    guard.turn = r["turn"]["id"].as_str().map(str::to_string);

    let mut out = Completion::default();
    let mut current_item: Option<String> = None;
    let mut error: Option<String> = None;
    loop {
        let Some(v) = rx.recv().await else {
            error = Some("codex app-server exited".into());
            p.kill();
            break;
        };
        let params = &v["params"];
        match v["method"].as_str().unwrap_or("") {
            "item/agentMessage/delta" => {
                let d = params["delta"].as_str().unwrap_or("");
                let item = params["itemId"].as_str().map(str::to_string);
                // A new message (e.g. after a tool call) starts a new paragraph.
                if item != current_item {
                    if !out.text.is_empty() {
                        out.text.push_str("\n\n");
                        on_delta("\n\n");
                    }
                    current_item = item;
                }
                out.text.push_str(d);
                on_delta(d);
            }
            "item/tool/call" => {
                let (name, args) = (params["tool"].as_str().unwrap_or("").to_string(), params["arguments"].clone());
                let (content, ok) = match t.tools {
                    Some((_, runner)) => (runner.0)(name, args).await,
                    None => ("error: no tools are available".to_string(), false),
                };
                p.respond(&v["id"], json!({ "contentItems": [{ "type": "inputText", "text": content }], "success": ok })).await;
            }
            "thread/tokenUsage/updated" => {
                let last = &params["tokenUsage"]["last"];
                out.usage.input_tokens += last["inputTokens"].as_u64().unwrap_or(0);
                out.usage.output_tokens += last["outputTokens"].as_u64().unwrap_or(0);
            }
            "error" if params["willRetry"] != true => {
                error = Some(params["error"]["message"].as_str().unwrap_or("codex error").to_string());
            }
            "turn/completed" => {
                let turn = &params["turn"];
                if turn["status"] == "failed" && error.is_none() {
                    error = Some(turn["error"]["message"].as_str().unwrap_or("codex turn failed").to_string());
                }
                guard.done = true;
                break;
            }
            // Codex runs with approvalPolicy=never; decline anything else it asks.
            _ if v.get("id").is_some() => {
                let _ = p.write(&json!({ "id": v["id"], "error": { "code": -32601, "message": "not supported by Alice" } })).await;
            }
            _ => {}
        }
    }
    drop(guard);
    Ok(match error {
        Some(e) => {
            // A login change is picked up by a fresh process.
            if e.contains("log in") || e.contains("401") || e.contains("auth") {
                reset().await;
            }
            Err(ProviderError(e))
        }
        None => Ok(out),
    })
}
