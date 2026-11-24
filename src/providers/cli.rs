//! Subscription access through the vendors' official CLIs.
//!
//! - `codex-cli`: `codex exec --json` using the user's ChatGPT login
//!   (`codex login`). Runs ephemeral and read-only in an empty directory.
//! - `gemini-cli`: `gemini -p --output-format stream-json` using the user's
//!   Google login. Experimental: availability of subscription logins for
//!   Gemini CLI has changed over time; prefer the Gemini API if it fails.
//!
//! Constraints (documented in ARCHITECTURE.md): Alice tools are not offered,
//! images/files are not forwarded, and output arrives per message rather
//! than per token. Alice still owns the conversation: the full transcript is
//! rendered into each prompt and the CLI keeps no session.

use super::*;
use crate::config::{ProviderConfig, ProviderKind};
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

pub fn render_prompt(req: &ChatRequest) -> String {
    let mut s = String::new();
    if !req.system.is_empty() {
        s.push_str("<instructions>\n");
        s.push_str(&req.system);
        s.push_str("\n</instructions>\n\n");
    }
    s.push_str("<conversation>\n");
    for m in &req.messages {
        let who = match m.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => "Tool result",
        };
        if m.content.is_empty() {
            continue;
        }
        s.push_str(&format!("[{who}]\n{}\n\n", m.content));
    }
    s.push_str("</conversation>\n\nReply to the last User message as the Assistant. \
Answer directly in text. Do not run commands, edit files, or read local files.");
    s
}

pub async fn stream(p: &ProviderConfig, req: &ChatRequest<'_>, on_delta: OnDelta<'_>) -> Result<Completion> {
    let prompt = render_prompt(req);
    let exe = p.base_url().to_string();
    let scratch = std::env::temp_dir().join(format!("alice-cli-{}", crate::util::new_id()));
    std::fs::create_dir_all(&scratch).map_err(|e| ProviderError(e.to_string()))?;
    let result = run(p.kind, &exe, req.model, &prompt, &scratch, on_delta).await;
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

async fn run(
    kind: ProviderKind,
    exe: &str,
    model: &str,
    prompt: &str,
    dir: &std::path::Path,
    on_delta: OnDelta<'_>,
) -> Result<Completion> {
    let mut cmd = Command::new(exe);
    cmd.current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    match kind {
        ProviderKind::CodexCli => {
            cmd.args(["exec", "--json", "--ephemeral", "--skip-git-repo-check", "--sandbox", "read-only", "--color", "never"]);
            cmd.arg("-C").arg(dir);
            if !model.is_empty() && model != "default" {
                cmd.args(["-m", model]);
            }
            cmd.arg("-");
        }
        ProviderKind::GeminiCli => {
            cmd.args(["--output-format", "stream-json"]);
            if !model.is_empty() && model != "default" {
                cmd.args(["-m", model]);
            }
            // Prompt is read from stdin and appended to -p.
            cmd.args(["-p", " "]);
        }
        _ => unreachable!(),
    }
    let mut child = cmd.spawn().map_err(|e| {
        ProviderError(format!("cannot start '{exe}': {e} (install the official CLI and log in)"))
    })?;
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| ProviderError(e.to_string()))?;
    }
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let mut out = Completion::default();
    let mut error: Option<String> = None;

    while let Some(line) = lines.next_line().await.map_err(|e| ProviderError(e.to_string()))? {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let ty = v["type"].as_str().unwrap_or("");
        match kind {
            ProviderKind::CodexCli => match ty {
                "item.completed" if v.pointer("/item/type").and_then(|t| t.as_str()) == Some("agent_message") => {
                    let t = v.pointer("/item/text").and_then(|t| t.as_str()).unwrap_or("");
                    if !out.text.is_empty() {
                        out.text.push_str("\n\n");
                        on_delta("\n\n");
                    }
                    out.text.push_str(t);
                    on_delta(t);
                }
                "turn.completed" => {
                    out.usage.input_tokens = v.pointer("/usage/input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                    out.usage.output_tokens = v.pointer("/usage/output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                }
                "turn.failed" | "error" => {
                    error = Some(
                        v.pointer("/error/message")
                            .or_else(|| v.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("codex failed")
                            .to_string(),
                    );
                }
                _ => {}
            },
            _ => match ty {
                // Field names are not formally documented; accept common shapes.
                "message" if v["role"] == "assistant" || v["role"] == "model" => {
                    let t = v["content"].as_str().or(v["text"].as_str()).unwrap_or("");
                    out.text.push_str(t);
                    on_delta(t);
                }
                "error" if v["severity"] != "warning" => {
                    error = Some(v["message"].as_str().or(v.pointer("/error/message").and_then(|m| m.as_str())).unwrap_or("gemini failed").to_string())
                }
                "result" => {
                    if let Some(s) = v.pointer("/stats/input_tokens").and_then(|n| n.as_u64()) {
                        out.usage.input_tokens = s;
                    }
                    if let Some(s) = v.pointer("/stats/output_tokens").and_then(|n| n.as_u64()) {
                        out.usage.output_tokens = s;
                    }
                }
                _ => {}
            },
        }
    }
    let status = child.wait().await.map_err(|e| ProviderError(e.to_string()))?;
    if let Some(e) = error {
        if e.contains("not supported when using Codex with a ChatGPT account") {
            return Err(ProviderError(format!(
                "{e} Use the model name `default` (Codex picks the model for your plan) or a model your ChatGPT plan offers."
            )));
        }
        return Err(ProviderError(e));
    }
    if !status.success() && out.text.is_empty() {
        let mut stderr = String::new();
        if let Some(mut e) = child.stderr.take() {
            use tokio::io::AsyncReadExt;
            let _ = e.read_to_string(&mut stderr).await;
        }
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
        return Err(ProviderError(format!("{exe} exited with {status}: {}", crate::util::truncate(&last, 300))));
    }
    out.stop_reason = "end_turn".into();
    Ok(out)
}

/// Subscription rate limits from the official Codex app-server
/// (`account/rateLimits/read`). Spends no tokens.
pub async fn codex_rate_limits(exe: &str) -> std::result::Result<Value, String> {
    let fut = async {
        let mut child = Command::new(exe)
            .arg("app-server")
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("cannot start '{exe}': {e}"))?;
        let mut stdin = child.stdin.take().unwrap();
        let reqs = [
            serde_json::json!({ "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "alice", "version": env!("CARGO_PKG_VERSION") } } }),
            serde_json::json!({ "method": "initialized" }),
            serde_json::json!({ "id": 2, "method": "account/rateLimits/read" }),
        ];
        for r in reqs {
            stdin.write_all(format!("{r}\n").as_bytes()).await.map_err(|e| e.to_string())?;
        }
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v["id"] == 2 {
                drop(stdin);
                let _ = child.kill().await;
                if let Some(e) = v.get("error") {
                    return Err(e["message"].as_str().unwrap_or("codex error").to_string());
                }
                return Ok(v["result"].clone());
            }
        }
        Err("codex app-server exited without an answer (is codex up to date and logged in?)".into())
    };
    tokio::time::timeout(std::time::Duration::from_secs(20), fut).await.map_err(|_| "codex did not answer".to_string())?
}

/// Reduce the app-server answer to what Alice shows.
pub fn summarize_rate_limits(v: &Value) -> Value {
    let rl = &v["rateLimits"];
    let window = |w: &Value| {
        if w.is_null() {
            return Value::Null;
        }
        serde_json::json!({
            "used_percent": w["usedPercent"],
            "window_minutes": w["windowDurationMins"],
            "resets_at": w["resetsAt"].as_i64().map(|s| s * 1000),
        })
    };
    serde_json::json!({
        "plan": rl["planType"],
        "primary": window(&rl["primary"]),
        "secondary": window(&rl["secondary"]),
        "limit_reached": rl["rateLimitReachedType"],
        "reset_credits": v.pointer("/rateLimitResetCredits/availableCount"),
    })
}
