//! `alice mcp`: a local stdio MCP server for MCP-capable CLIs (Codex).
//!
//! It is only a relay. It holds no secrets and executes nothing itself:
//! `tools/list` and `tools/call` are forwarded to the running Alice server
//! with the per-turn token from `ALICE_MCP_TOKEN`, where the Tool Gate
//! applies permissions and approval. The token works only for `/api/mcp/*`
//! and only while that turn runs.

use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

const PROTOCOL: &str = "2025-06-18";

struct Relay {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Relay {
    async fn get(&self, path: &str) -> Result<Value, String> {
        let r = self.http.get(format!("{}/api{path}", self.base)).header("x-alice-turn", &self.token).send().await.map_err(|e| e.without_url().to_string())?;
        Self::json(r).await
    }
    async fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let r = self
            .http
            .post(format!("{}/api{path}", self.base))
            .header("x-alice-turn", &self.token)
            .json(body)
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        Self::json(r).await
    }
    async fn json(r: reqwest::Response) -> Result<Value, String> {
        let ok = r.status().is_success();
        let v: Value = r.json().await.map_err(|e| e.without_url().to_string())?;
        if ok { Ok(v) } else { Err(v["error"].as_str().unwrap_or("request failed").to_string()) }
    }
}

pub async fn serve_stdio() -> Result<(), String> {
    let token = std::env::var("ALICE_MCP_TOKEN").map_err(|_| "alice mcp is started by Alice for one turn (ALICE_MCP_TOKEN is not set)")?;
    let base = std::env::var("ALICE_URL").unwrap_or_else(|_| "http://127.0.0.1:7878".into());
    if !(base.starts_with("http://127.0.0.1:") || base.starts_with("http://localhost:") || base.starts_with("http://[::1]:")) {
        return Err("alice mcp only talks to a local Alice server".into());
    }
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(900))
        .build()
        .map_err(|e| e.to_string())?;
    let relay = Arc::new(Relay { http, base: base.trim_end_matches('/').to_string(), token });
    let out = Arc::new(Mutex::new(tokio::io::stdout()));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else { continue }; // notifications
        let (relay, out) = (relay.clone(), out.clone());
        // Calls may wait minutes for approval; answer each request independently.
        tokio::spawn(async move {
            let reply = match handle(&relay, &msg).await {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
            };
            let mut o = out.lock().await;
            let _ = o.write_all(format!("{reply}\n").as_bytes()).await;
            let _ = o.flush().await;
        });
    }
    Ok(())
}

async fn handle(relay: &Relay, msg: &Value) -> Result<Value, (i64, String)> {
    let params = &msg["params"];
    match msg["method"].as_str().unwrap_or("") {
        "initialize" => {
            let instructions = relay.get("/mcp/tools").await.ok().and_then(|v| v["instructions"].as_str().map(str::to_string)).unwrap_or_default();
            Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "alice", "version": env!("CARGO_PKG_VERSION") },
                "instructions": instructions,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let v = relay.get("/mcp/tools").await.map_err(|e| (-32000, e))?;
            Ok(json!({ "tools": v["tools"] }))
        }
        "tools/call" => {
            let name = params["name"].as_str().ok_or((-32602, "missing tool name".to_string()))?;
            let body = json!({ "name": name, "arguments": params.get("arguments").cloned().unwrap_or(json!({})) });
            match relay.post("/mcp/call", &body).await {
                Ok(v) => Ok(json!({
                    "content": [{ "type": "text", "text": v["content"].as_str().unwrap_or("") }],
                    "isError": v["ok"] != true,
                })),
                Err(e) => Ok(json!({ "content": [{ "type": "text", "text": format!("error: {e}") }], "isError": true })),
            }
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        m => Err((-32601, format!("method not found: {m}"))),
    }
}
