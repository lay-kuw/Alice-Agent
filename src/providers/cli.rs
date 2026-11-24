//! Subscription access through the vendors' official CLIs.
//!
//! - `codex-cli`: `codex exec --json` with Alice's own CODEX_HOME and login
//!   (`alice model login codex-cli`). Runs ephemeral, read-only, in an empty
//!   directory, with Codex's shell/exec/browser/plugins turned off. Alice's
//!   tools are reached through `alice mcp` (see docs/SKILLS.md), after a
//!   safety check of the tools Codex exposes.
//! - `gemini-cli`: `gemini -p --output-format stream-json` using the user's
//!   Google login. Experimental: availability of subscription logins for
//!   Gemini CLI has changed over time; prefer the Gemini API if it fails.
//!
//! Other CLIs (and Codex if the safety check fails) get Alice's tools through
//! a text protocol (```alice-tool blocks). Images are not forwarded and
//! output arrives per message. Alice owns the conversation: the transcript is
//! rendered into each prompt and the CLI keeps no session.

use super::*;
use crate::config::{ProviderConfig, ProviderKind};
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

// ------------------------------------------------------------------ Codex home

static CODEX_HOME: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);

/// Alice runs Codex with its own CODEX_HOME so the user's ~/.codex (plugins,
/// MCP servers, hooks, history, memories) is never loaded. Login is separate.
pub fn set_codex_home(p: std::path::PathBuf) {
    *CODEX_HOME.write().unwrap_or_else(|e| e.into_inner()) = Some(p);
}

pub fn codex_home() -> Option<std::path::PathBuf> {
    CODEX_HOME.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Codex features that give the model capabilities outside Alice's gate.
const CODEX_OFF: &[&str] = &[
    "shell_tool", "unified_exec", "unified_exec_tty", "browser_use", "browser_use_external", "computer_use",
    "in_app_browser", "in_app_chat", "in_app_local_automation", "image_generation", "apps", "plugins", "remote_plugin",
    "hooks", "goals", "multi_agent", "skill_search", "skill_mcp_dependency_install", "view_image", "tool_suggest",
    "shell_snapshot", "workspace_dependencies", "code_mode_host",
];

const CODEX_CONFIG_HEAD: &str = "# Written by Alice before every Codex run. Changes here are overwritten.\n\
approval_policy = \"never\"\nsandbox_mode = \"read-only\"\nweb_search = \"disabled\"\n\
include_apply_patch_tool = false\n\n[history]\npersistence = \"none\"\n\n[features]\n";

/// Create Alice's CODEX_HOME and (re)write its config.toml.
pub fn prepare_codex_home() -> std::result::Result<std::path::PathBuf, String> {
    let home = codex_home().ok_or("codex home is not set")?;
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700));
    }
    let mut cfg = CODEX_CONFIG_HEAD.to_string();
    for f in CODEX_OFF {
        cfg.push_str(&format!("{f} = false\n"));
    }
    let path = home.join("config.toml");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(cfg.as_str()) {
        crate::config::write_atomic(&path, cfg.as_bytes(), 0o600)?;
    }
    Ok(home)
}

/// `codex` with Alice's CODEX_HOME (tokio).
pub fn codex_cmd(exe: &str) -> Command {
    let mut c = Command::new(exe);
    if let Ok(h) = prepare_codex_home() {
        c.env("CODEX_HOME", h);
    }
    c
}

/// `-c` overrides that add `alice mcp` as Codex's only MCP server. The
/// per-turn token is passed through the environment, not the command line.
fn mcp_overrides(b: &McpBridge) -> Vec<String> {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "alice".into());
    let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
    vec![
        format!("mcp_servers.alice.command={}", q(&exe)),
        format!("mcp_servers.alice.args=[{}, {}, {}]", q("-p"), q(&b.profile), q("mcp")),
        "mcp_servers.alice.env_vars=[\"ALICE_MCP_TOKEN\", \"ALICE_URL\"]".into(),
        "mcp_servers.alice.default_tools_approval_mode=\"approve\"".into(),
        "mcp_servers.alice.tool_timeout_sec=900".into(),
        "mcp_servers.alice.startup_timeout_sec=20".into(),
    ]
}

// ------------------------------------------------------------------ safety check

/// Tools the model may see in Alice's Codex. Anything else (a shell, file
/// readers, browsers, new built-ins in a future Codex) disables the MCP bridge.
const CODEX_ALLOWED: &[&str] = &[
    "functions", "exec", "wait", "request_user_input", "request_user_input_async", "send_message_to_user_async",
    "collaboration", "spawn_agent", "followup_task", "interrupt_agent", "list_agents", "send_message", "wait_agent",
    "clock", "sleep", "curr_time", "clock__curr_time", "apply_patch", "list_mcp_resource_templates", "list_mcp_resources",
    "read_mcp_resource",
];

fn tool_allowed(name: &str) -> bool {
    CODEX_ALLOWED.contains(&name) || name.starts_with("mcp__alice__") || name.starts_with("alice__")
}

/// Names of every tool in `codex debug prompt-input` output, including tools
/// nested inside the `exec` code-mode tool.
pub fn codex_tool_names(v: &Value) -> Vec<String> {
    fn walk(x: &Value, out: &mut Vec<String>) {
        match x {
            Value::Object(m) => {
                if let Some(Value::String(n)) = m.get("name")
                    && (m.contains_key("parameters") || m.contains_key("description") || m.contains_key("tools")) {
                        out.push(n.clone());
                    }
                if m.get("name").and_then(|n| n.as_str()) == Some("exec")
                    && let Some(d) = m.get("description").and_then(|d| d.as_str()) {
                        for part in d.split("### `").skip(1) {
                            if let Some(n) = part.split('`').next() {
                                out.push(n.to_string());
                            }
                        }
                    }
                for v in m.values() {
                    walk(v, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, &mut out);
    out.sort();
    out.dedup();
    out
}

static CHECK: std::sync::Mutex<Option<(String, std::result::Result<(), String>)>> = std::sync::Mutex::new(None);

/// Verify that Alice's Codex exposes no tool outside the allowlist. Cached
/// per Codex binary (path + modification time). Spends no tokens.
pub async fn codex_safe(exe: &str) -> std::result::Result<(), String> {
    let resolved = which(exe).unwrap_or_else(|| std::path::PathBuf::from(exe));
    let real = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    let key = format!("{}:{:?}", real.display(), std::fs::metadata(&real).and_then(|m| m.modified()).ok());
    if let Some((k, r)) = CHECK.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && *k == key {
            return r.clone();
        }
    let dir = std::env::temp_dir();
    let out = codex_cmd(exe)
        .args(["debug", "prompt-input", "-c", "web_search=\"disabled\"", "check"])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let r = match tokio::time::timeout(std::time::Duration::from_secs(30), out).await {
        Ok(Ok(o)) if o.status.success() => match serde_json::from_slice::<Value>(&o.stdout) {
            Ok(v) => {
                let bad: Vec<String> = codex_tool_names(&v).into_iter().filter(|n| !tool_allowed(n)).collect();
                if bad.is_empty() { Ok(()) } else { Err(format!("Codex exposes tools outside Alice's control: {}", bad.join(", "))) }
            }
            Err(e) => Err(format!("cannot read Codex tool list: {e}")),
        },
        Ok(Ok(o)) => Err(format!("`codex debug prompt-input` failed ({})", o.status)),
        Ok(Err(e)) => Err(format!("cannot run codex: {e}")),
        Err(_) => Err("codex did not answer".into()),
    };
    match &r {
        Ok(()) => crate::info!("codex safety check passed"),
        Err(e) => crate::warn!("codex safety check failed, tools use the text protocol: {e}"),
    }
    *CHECK.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, r.clone()));
    r
}

fn which(exe: &str) -> Option<std::path::PathBuf> {
    if exe.contains('/') {
        return Some(exe.into());
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(exe)).find(|p| p.is_file())
}

// ------------------------------------------------------------------ prompt

/// How tools reach this run.
#[derive(PartialEq, Eq, Clone, Copy)]
enum ToolMode {
    None,
    Mcp,
    Text,
}

pub fn render_prompt(req: &ChatRequest) -> String {
    render(req, if req.tools.is_empty() { ToolMode::None } else { ToolMode::Text })
}

fn render(req: &ChatRequest, mode: ToolMode) -> String {
    let mut s = String::new();
    if !req.system.is_empty() {
        s.push_str("<instructions>\n");
        s.push_str(&req.system);
        s.push_str("\n</instructions>\n\n");
    }
    if mode == ToolMode::Text {
        s.push_str(&text_protocol(&req.tools));
    }
    s.push_str("<conversation>\n");
    for m in &req.messages {
        let who = match m.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => "Tool result",
        };
        if m.content.is_empty() && m.tool_calls.is_empty() {
            continue;
        }
        s.push_str(&format!("[{who}]\n{}\n", m.content));
        for c in &m.tool_calls {
            s.push_str(&format!("```alice-tool\n{}\n```\n", json!({ "name": c.name, "arguments": c.arguments })));
        }
        s.push('\n');
    }
    s.push_str("</conversation>\n\nReply to the last User message as the Assistant.");
    s.push_str(match mode {
        ToolMode::Mcp => " Use the `alice` MCP tools when an action or lookup is needed; they are the only tools you may use. \
Do not run commands, edit files, or read local files.",
        ToolMode::Text => " To use a tool, follow the tool protocol above and stop; otherwise answer directly in text. \
Do not run commands, edit files, or read local files.",
        ToolMode::None => " Answer directly in text. Do not run commands, edit files, or read local files.",
    });
    s
}

fn text_protocol(tools: &[ToolSpec]) -> String {
    let mut s = String::from(
        "<tools>\nYou can call these tools. To call one, write a fenced block and then STOP writing; \
the result will come back as a [Tool result]:\n```alice-tool\n{\"name\": \"<tool>\", \"arguments\": {…}}\n```\n\
Several blocks may be written together. Use exact names and JSON arguments matching the schema.\n\n",
    );
    for t in tools {
        s.push_str(&format!("- {}: {}\n  schema: {}\n", t.name, t.description, t.parameters));
    }
    s.push_str("</tools>\n\n");
    s
}

/// Pull ```alice-tool blocks out of a text reply.
pub fn parse_text_calls(text: &str) -> (String, Vec<ToolCall>) {
    let mut calls = Vec::new();
    let mut kept = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("```alice-tool") {
        kept.push_str(&rest[..i]);
        let after = &rest[i + "```alice-tool".len()..];
        let Some(end) = after.find("```") else {
            rest = "";
            break;
        };
        let body = after[..end].trim();
        if let Ok(v) = serde_json::from_str::<Value>(body)
            && let Some(name) = v["name"].as_str() {
                calls.push(ToolCall {
                    id: format!("text_{}", crate::util::new_id()),
                    name: name.to_string(),
                    arguments: if v["arguments"].is_object() { v["arguments"].clone() } else { json!({}) },
                });
            }
        rest = &after[end + 3..];
    }
    kept.push_str(rest);
    (kept.trim().to_string(), calls)
}

pub async fn stream(p: &ProviderConfig, req: &ChatRequest<'_>, on_delta: OnDelta<'_>) -> Result<Completion> {
    let exe = p.base_url().to_string();
    let mode = match (&req.mcp, req.tools.is_empty(), p.kind) {
        (_, true, _) => ToolMode::None,
        (Some(_), false, ProviderKind::CodexCli) if codex_safe(&exe).await.is_ok() => ToolMode::Mcp,
        _ => ToolMode::Text,
    };
    let prompt = render(req, mode);
    let scratch = std::env::temp_dir().join(format!("alice-cli-{}", crate::util::new_id()));
    std::fs::create_dir_all(&scratch).map_err(|e| ProviderError(e.to_string()))?;
    let bridge = if mode == ToolMode::Mcp { req.mcp.as_ref() } else { None };
    // Text-protocol replies are buffered so tool blocks never reach the UI.
    let mut buf = String::new();
    let mut sink = |d: &str| buf.push_str(d);
    let result = if mode == ToolMode::Text {
        run(p.kind, &exe, req.model, &prompt, &scratch, bridge, &mut sink).await
    } else {
        run(p.kind, &exe, req.model, &prompt, &scratch, bridge, on_delta).await
    };
    let _ = std::fs::remove_dir_all(&scratch);
    let mut out = result?;
    if mode == ToolMode::Text {
        let (text, calls) = parse_text_calls(&out.text);
        if !text.is_empty() {
            on_delta(&text);
        }
        out.text = text;
        if !calls.is_empty() {
            out.stop_reason = "tool_use".into();
        }
        out.tool_calls = calls;
    }
    Ok(out)
}

async fn run(
    kind: ProviderKind,
    exe: &str,
    model: &str,
    prompt: &str,
    dir: &std::path::Path,
    bridge: Option<&McpBridge>,
    on_delta: OnDelta<'_>,
) -> Result<Completion> {
    let mut cmd = match kind {
        ProviderKind::CodexCli => codex_cmd(exe),
        _ => Command::new(exe),
    };
    cmd.current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    match kind {
        ProviderKind::CodexCli => {
            cmd.args(["exec", "--json", "--ephemeral", "--skip-git-repo-check", "--sandbox", "read-only", "--color", "never"]);
            cmd.arg("-C").arg(dir);
            if let Some(b) = bridge {
                for o in mcp_overrides(b) {
                    cmd.arg("-c").arg(o);
                }
                cmd.env("ALICE_MCP_TOKEN", &b.token).env("ALICE_URL", &b.url);
            }
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
        let mut child = codex_cmd(exe)
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
