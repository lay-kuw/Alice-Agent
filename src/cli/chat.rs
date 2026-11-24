//! `alice chat`: interactive and one-shot chat over the server API.

use super::client::{CliResult, Client};
use super::markdown::Md;
use crate::util::fmt_utc;
use serde_json::{Value, json};
use std::io::{IsTerminal, Write};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, mpsc};

static STREAMING: AtomicBool = AtomicBool::new(false);
static STOP: LazyLock<Notify> = LazyLock::new(Notify::new);

/// Ctrl-C stops a streaming response; otherwise it exits.
pub fn install_ctrl_c() {
    tokio::spawn(async {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            if STREAMING.load(Ordering::SeqCst) {
                STOP.notify_one();
            } else {
                println!();
                std::process::exit(130);
            }
        }
    });
}

struct StreamingGuard;
impl StreamingGuard {
    fn new() -> StreamingGuard {
        STREAMING.store(true, Ordering::SeqCst);
        StreamingGuard
    }
}
impl Drop for StreamingGuard {
    fn drop(&mut self) {
        STREAMING.store(false, Ordering::SeqCst);
    }
}

pub const DIM: &str = "\x1b[2m";
pub const RESET: &str = "\x1b[0m";
pub const RED: &str = "\x1b[31m";
pub const YELLOW: &str = "\x1b[33m";

pub fn color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

pub fn dim(s: &str) -> String {
    if color() { format!("{DIM}{s}{RESET}") } else { s.to_string() }
}

fn paint(code: &str, s: &str) -> String {
    if color() { format!("{code}{s}{RESET}") } else { s.to_string() }
}
pub fn red(s: &str) -> String {
    paint(RED, s)
}
pub fn yellow(s: &str) -> String {
    paint(YELLOW, s)
}
pub fn green(s: &str) -> String {
    paint("\x1b[32m", s)
}

/// Lines from stdin, read on a background thread so prompts can be
/// answered while events stream in.
pub struct Input {
    rx: mpsc::UnboundedReceiver<Option<String>>,
}

impl Input {
    pub fn new() -> Input {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            loop {
                let mut s = String::new();
                match stdin.read_line(&mut s) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(None);
                        break;
                    }
                    Ok(_) => {
                        if tx.send(Some(s.trim_end_matches(['\n', '\r']).to_string())).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Input { rx }
    }

    pub async fn line(&mut self, prompt: &str) -> Option<String> {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        self.rx.recv().await.flatten()
    }

    /// Read input; a trailing `\` continues on the next line.
    pub async fn message(&mut self, prompt: &str) -> Option<String> {
        let mut text = self.line(prompt).await?;
        while text.ends_with('\\') {
            text.pop();
            text.push('\n');
            text.push_str(&self.line("  ").await?);
        }
        Some(text)
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

fn args_summary(v: &Value) -> String {
    let s = match v {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| {
                let val = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("{k}={}", crate::util::truncate(&val.replace('\n', " "), 60))
            })
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    };
    crate::util::truncate(&s, 160).to_string()
}

/// Send one message and render the streamed response until the turn ends.
pub async fn send_and_stream(
    client: &Client,
    input: &mut Input,
    thread_id: &str,
    text: &str,
    model: Option<&str>,
    attachments: &[String],
) -> CliResult<()> {
    let mut es = client.events(Some(thread_id)).await?;
    client
        .post(
            &format!("/threads/{thread_id}/messages"),
            &json!({ "content": text, "client": "cli", "model": model, "attachments": attachments }),
        )
        .await?;
    let mut md = Md::new(color());
    let mut stopping = false;
    let _guard = StreamingGuard::new();
    loop {
        let ev = tokio::select! {
            ev = es.next() => ev?,
            _ = STOP.notified(), if !stopping => {
                stopping = true;
                let _ = client.post(&format!("/threads/{thread_id}/stop"), &json!({})).await;
                continue;
            }
        };
        match ev["type"].as_str().unwrap_or("") {
            "message.delta" => {
                md.push(ev["delta"].as_str().unwrap_or(""));
                md.flush();
            }
            "message.completed" => {
                let m = &ev["message"];
                if m["role"] != "assistant" {
                    continue;
                }
                md.finish();
                let has_tools = m["tool_calls"].as_array().is_some_and(|a| !a.is_empty()) && m["meta"]["final"] != true;
                match m["status"].as_str().unwrap_or("") {
                    "cancelled" => {
                        println!("{}", dim("(stopped)"));
                        return Ok(());
                    }
                    "error" => {
                        let err = m.pointer("/meta/error").and_then(|e| e.as_str()).unwrap_or("generation failed");
                        if color() {
                            eprintln!("{RED}error:{RESET} {err}");
                        } else {
                            eprintln!("error: {err}");
                        }
                        return Ok(());
                    }
                    _ if has_tools => {}
                    _ => {
                        let meta = &m["meta"];
                        let model = m.pointer("/meta/model_name").and_then(|n| n.as_str()).or(m["model"].as_str()).unwrap_or("");
                        let pms = meta["provider_ms"].as_u64().unwrap_or(0);
                        let ams = meta["alice_ms"].as_u64().unwrap_or(0);
                        println!("{}", dim(&format!("— {model} · {:.1}s (alice {ams}ms)", pms as f64 / 1000.0)));
                        return Ok(());
                    }
                }
            }
            "tool.started" => {
                md.finish();
                println!("{}", dim(&format!("⚙ {} {}", ev["tool"].as_str().unwrap_or(""), args_summary(&ev["arguments"]))));
            }
            "tool.completed" => {
                if ev["ok"] == false {
                    println!("{}", dim(&format!("  ✗ {}", crate::util::truncate(ev["summary"].as_str().unwrap_or(""), 200))));
                }
            }
            "tool.approval_required" => {
                md.finish();
                let tool = ev["tool"].as_str().unwrap_or("");
                let q = format!("Allow {tool} {}? [y/N] ", args_summary(&ev["arguments"]));
                let prompt = if color() { format!("{YELLOW}?{RESET} {q}") } else { format!("? {q}") };
                let ans = input.line(&prompt).await.unwrap_or_default();
                let approve = matches!(ans.trim().to_lowercase().as_str(), "y" | "yes");
                let id = ev["approval_id"].as_str().unwrap_or("");
                // Another client may have answered first; ignore 404.
                let _ = client.post(&format!("/approvals/{id}"), &json!({ "approve": approve })).await;
            }
            "thread.deleted" | "threads.cleared" => {
                md.finish();
                println!("{}", dim("(this thread was deleted)"));
                return Err("thread deleted".into());
            }
            _ => {}
        }
    }
}

pub async fn create_thread(client: &Client, model: Option<&str>) -> CliResult<String> {
    let v = client.post("/threads", &json!({ "model": model })).await?;
    v.pointer("/thread/id").and_then(|i| i.as_str()).map(str::to_string).ok_or("bad response".into())
}

pub async fn one_shot(client: &Client, text: &str) -> CliResult<()> {
    install_ctrl_c();
    let id = create_thread(client, None).await?;
    let mut input = Input::new();
    send_and_stream(client, &mut input, &id, text, None, &[]).await?;
    println!("{}", dim(&format!("thread {id} · continue with `alice chat {id}`")));
    Ok(())
}

pub async fn list(client: &Client) -> CliResult<()> {
    let v = client.get("/threads").await?;
    let threads = v["threads"].as_array().cloned().unwrap_or_default();
    if threads.is_empty() {
        println!("No threads.");
        return Ok(());
    }
    for t in threads {
        let title = t["title"].as_str().filter(|s| !s.is_empty()).unwrap_or("(untitled)");
        let when = fmt_utc(t["updated_at"].as_i64().unwrap_or(0));
        let busy = if t["generating"] == true { " ●" } else { "" };
        println!("{}  {}  {}{}", t["id"].as_str().unwrap_or(""), dim(&when[..16]), title, busy);
    }
    Ok(())
}

fn print_history(v: &Value) {
    let msgs = v["messages"].as_array().cloned().unwrap_or_default();
    let start = msgs.len().saturating_sub(20);
    if start > 0 {
        println!("{}", dim(&format!("… {start} earlier messages")));
    }
    for m in &msgs[start..] {
        match m["role"].as_str().unwrap_or("") {
            "user" => println!("{} {}", if color() { "\x1b[1m›\x1b[0m" } else { ">" }, m["content"].as_str().unwrap_or("")),
            "assistant" => {
                let c = m["content"].as_str().unwrap_or("");
                if !c.is_empty() {
                    let mut md = Md::new(color());
                    md.push(c);
                    md.finish();
                }
            }
            _ => {}
        }
    }
}

const HELP: &str = "Commands:
  /new              start a new thread
  /list             list threads
  /open <id>        switch to a thread
  /model [id]       show or set this thread's model
  /rename <title>   rename this thread
  /delete           delete this thread (and memories derived from it)
  /attach <path>    attach a file to the next message
  /usage            subscription limits and tokens used
  /help             this help
  /exit             quit
End a line with \\ to continue on the next line. Ctrl-C stops a response.";

pub async fn interactive(client: &Client, thread: Option<String>) -> CliResult<()> {
    install_ctrl_c();
    let mut input = Input::new();
    let mut thread_id = thread;
    let mut model: Option<String> = None;
    let mut pending: Vec<String> = Vec::new();
    let status = client.get("/models").await?;
    let default_model = status["default"].as_str().unwrap_or("").to_string();
    if let Some(id) = &thread_id {
        let v = client.get(&format!("/threads/{id}/messages")).await?;
        let t = client.get(&format!("/threads/{id}")).await?;
        model = t.pointer("/thread/model").and_then(|m| m.as_str()).map(str::to_string);
        println!("{}", dim(&format!("thread {id} · {}", t.pointer("/thread/title").and_then(|s| s.as_str()).unwrap_or(""))));
        print_history(&v);
    }
    println!(
        "{}",
        dim(&format!("Alice · {} · /help for commands", model.as_deref().unwrap_or(if default_model.is_empty() { "no model configured" } else { &default_model })))
    );
    loop {
        let prompt = if color() { "\x1b[1m›\x1b[0m " } else { "> " };
        let Some(line) = input.message(prompt).await else { break };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Some(cmd) = line.strip_prefix('/') {
            let (name, arg) = cmd.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((cmd, ""));
            let r: CliResult<()> = async {
                match name {
                    "exit" | "quit" | "q" => return Err("__exit".into()),
                    "help" | "?" => println!("{HELP}"),
                    "new" => {
                        thread_id = None;
                        pending.clear();
                        println!("{}", dim("new thread"));
                    }
                    "list" | "ls" => list(client).await?,
                    "usage" => print!("{}", crate::usage::render_text(&client.get("/usage").await?)),
                    "open" | "switch" => {
                        let v = client.get(&format!("/threads/{arg}")).await?;
                        let id = v.pointer("/thread/id").and_then(|i| i.as_str()).unwrap_or(arg).to_string();
                        model = v.pointer("/thread/model").and_then(|m| m.as_str()).map(str::to_string);
                        let msgs = client.get(&format!("/threads/{id}/messages")).await?;
                        print_history(&msgs);
                        thread_id = Some(id);
                    }
                    "model" => {
                        if arg.is_empty() {
                            println!("{}", model.as_deref().unwrap_or(&default_model));
                        } else {
                            model = Some(arg.to_string());
                            if let Some(id) = &thread_id {
                                client.patch(&format!("/threads/{id}"), &json!({ "model": arg })).await?;
                            }
                            println!("{}", dim(&format!("model: {arg}")));
                        }
                    }
                    "rename" => {
                        let id = thread_id.as_deref().ok_or("no thread yet")?;
                        client.patch(&format!("/threads/{id}"), &json!({ "title": arg })).await?;
                    }
                    "delete" => {
                        let id = thread_id.take().ok_or("no thread yet")?;
                        client.delete(&format!("/threads/{id}"), None).await?;
                        println!("{}", dim("deleted"));
                    }
                    "attach" => {
                        let path = std::path::Path::new(arg);
                        let bytes = std::fs::read(path).map_err(|e| format!("{arg}: {e}"))?;
                        if thread_id.is_none() {
                            thread_id = Some(create_thread(client, model.as_deref()).await?);
                        }
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                        let v = client.upload(thread_id.as_deref().unwrap(), name, bytes).await?;
                        pending.push(v["id"].as_str().unwrap_or("").to_string());
                        println!("{}", dim(&format!("attached {name}")));
                    }
                    _ => println!("unknown command; /help"),
                }
                Ok(())
            }
            .await;
            match r {
                Err(e) if e == "__exit" => break,
                Err(e) => eprintln!("error: {e}"),
                Ok(()) => {}
            }
            continue;
        }
        if thread_id.is_none() {
            match create_thread(client, model.as_deref()).await {
                Ok(id) => thread_id = Some(id),
                Err(e) => {
                    eprintln!("error: {e}");
                    continue;
                }
            }
        }
        let id = thread_id.clone().unwrap();
        let atts = std::mem::take(&mut pending);
        match send_and_stream(client, &mut input, &id, &line, None, &atts).await {
            Ok(()) => {}
            Err(e) if e == "thread deleted" => thread_id = None,
            Err(e) => eprintln!("error: {e}"),
        }
    }
    Ok(())
}
