//! `alice` command line. Everything except `serve` is a client of the
//! server API; settings commands edit the config files directly only when
//! the server is not running (so `alice setup` / `alice model` work first).

pub mod chat;
pub mod client;
pub mod markdown;
pub mod service;

use crate::config::{Config, Paths, Permission, ProviderConfig, ProviderKind, Secrets};
use chat::dim;
use clap::{Parser, Subcommand};
use client::{CliResult, Client};
use serde_json::{Value, json};
use std::io::Write;

#[derive(Parser)]
#[command(name = "alice", version, about = "Alice Agent (A2) — a lightweight personal AI agent", disable_help_subcommand = true)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Interactive first-time setup
    Setup,
    /// Start the server (service or background process)
    Start,
    /// Stop the server
    Stop,
    /// Restart the server
    Restart,
    /// Show server status
    Status,
    /// Show the server log
    Log {
        #[arg(short = 'f', long)]
        follow: bool,
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
    },
    /// Chat: `alice chat`, `alice chat "message"`, `alice chat new|list|<id>|delete <id>|rename <id>|clear`
    Chat { args: Vec<String> },
    /// Manage models: list, add, remove <id>, use <id>, login <provider>, logout <provider>, test <id>
    Model { args: Vec<String> },
    /// Memory: list, search "keyword", status, clear
    Memory { args: Vec<String> },
    /// Config: `alice config`, `get <key>`, `set <key> <value>`, `secret <name>`
    Config { args: Vec<String> },
    /// Tools: list, enable <tool> [ask|allow], disable <tool>
    Tool { args: Vec<String> },
    /// Telegram / Discord gateways: setup, list, delete <gw-id>
    Gateway { args: Vec<String> },
    /// Show the API token and connection info for Web / iPhone (`--rotate` to replace it)
    Token {
        #[arg(long)]
        rotate: bool,
    },
    /// Run the server in the foreground (used by the service manager)
    #[command(hide = true)]
    Serve,
}

pub async fn run(cmd: Option<Cmd>, paths: Paths) -> CliResult<()> {
    match cmd {
        None => {
            if !paths.config().exists() {
                println!("Welcome to Alice. Run `alice setup` to get started, or `alice --help`.");
                return Ok(());
            }
            let c = Client::from_env(&paths)?;
            chat::interactive(&c, None).await
        }
        Some(Cmd::Setup) => setup(&paths).await,
        Some(Cmd::Start) => service::start(&paths).await,
        Some(Cmd::Stop) => service::stop(&paths).await,
        Some(Cmd::Restart) => {
            let c = Client::from_env(&paths)?;
            if c.is_up().await {
                service::stop(&paths).await?;
            }
            service::start(&paths).await
        }
        Some(Cmd::Status) => status(&paths).await,
        Some(Cmd::Log { follow, lines }) => service::log(&paths, lines, follow),
        Some(Cmd::Chat { args }) => chat_cmd(&paths, args).await,
        Some(Cmd::Model { args }) => model_cmd(&paths, args).await,
        Some(Cmd::Memory { args }) => memory_cmd(&paths, args).await,
        Some(Cmd::Config { args }) => config_cmd(&paths, args).await,
        Some(Cmd::Tool { args }) => tool_cmd(&paths, args).await,
        Some(Cmd::Gateway { args }) => crate::channels::gateway_cmd(&paths, args).await,
        Some(Cmd::Token { rotate }) => token_cmd(&paths, rotate).await,
        Some(Cmd::Serve) => unreachable!(),
    }
}

// ---------------------------------------------------------------- prompts

pub fn ask(prompt: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{prompt}: ");
    } else {
        print!("{prompt} [{default}]: ");
    }
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    let s = s.trim();
    if s.is_empty() { default.to_string() } else { s.to_string() }
}

pub fn ask_secret(prompt: &str) -> String {
    print!("{prompt}: ");
    let _ = std::io::stdout().flush();
    use std::io::IsTerminal;
    let tty = std::io::stdin().is_terminal()
        && std::process::Command::new("stty")
            .arg("-echo")
            .stdin(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    if tty {
        let _ = std::process::Command::new("stty").arg("echo").stdin(std::process::Stdio::inherit()).status();
        println!();
    }
    s.trim().to_string()
}

pub fn confirm(prompt: &str, default: bool) -> bool {
    let a = ask(&format!("{prompt} ({})", if default { "Y/n" } else { "y/N" }), "");
    match a.to_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        _ => false,
    }
}

fn choose(prompt: &str, options: &[&str], default: usize) -> usize {
    for (i, o) in options.iter().enumerate() {
        println!("  {}) {o}", i + 1);
    }
    loop {
        let a = ask(prompt, &(default + 1).to_string());
        if let Ok(n) = a.parse::<usize>()
            && n >= 1 && n <= options.len() {
                return n - 1;
            }
    }
}

// ---------------------------------------------------------------- backend

/// Settings go through the API when the server runs, else to the files.
enum Backend {
    Remote(Client),
    Local,
}

async fn backend(paths: &Paths) -> Backend {
    match Client::from_env(paths) {
        Ok(c) if c.is_up().await => Backend::Remote(c),
        _ => Backend::Local,
    }
}

async fn remote(paths: &Paths) -> CliResult<Client> {
    let c = Client::from_env(paths)?;
    if !c.is_up().await {
        return Err(format!("cannot reach the Alice server at {} (run `alice start`)", c.base));
    }
    Ok(c)
}

fn local_add_provider(paths: &Paths, p: ProviderConfig, api_key: Option<&str>) -> CliResult<String> {
    let mut cfg = Config::load(paths)?;
    let first = format!("{}/{}", p.id, p.models[0]);
    match cfg.providers.iter_mut().find(|x| x.id == p.id) {
        Some(x) => *x = p.clone(),
        None => cfg.providers.push(p.clone()),
    }
    if cfg.agent.default_model.is_empty() || cfg.resolve_model(&cfg.agent.default_model).is_none() {
        cfg.agent.default_model = first;
    }
    cfg.save(paths)?;
    if let Some(k) = api_key {
        let mut s = Secrets::load(paths)?;
        s.set(&format!("provider.{}.api_key", p.id), k);
        s.save(paths)?;
    }
    Ok(cfg.agent.default_model)
}

// ---------------------------------------------------------------- setup

async fn setup(paths: &Paths) -> CliResult<()> {
    println!("Alice Agent setup\n");
    println!("1. Data directory: {}", paths.home.display());
    paths.ensure().map_err(|e| e.to_string())?;
    let mut cfg = Config::load(paths)?;

    println!("\n2. Server");
    println!("   Keep 127.0.0.1 unless you need LAN access; for remote use prefer Tailscale or an HTTPS reverse proxy.");
    cfg.server.bind = ask("   Bind address", &cfg.server.bind);
    loop {
        let p = ask("   Port", &cfg.server.port.to_string());
        if let Ok(n) = p.parse::<u16>() {
            cfg.server.port = n;
            break;
        }
    }
    cfg.server.public_url = ask("   Public URL for other devices (optional, e.g. https://host.tailnet.ts.net)", &cfg.server.public_url);
    cfg.agent.language = ask("   Default reply language", &cfg.agent.language);
    cfg.save(paths)?;

    println!("\n3. Authentication");
    let mut secrets = Secrets::load(paths)?;
    if secrets.get("server.token").is_none() {
        secrets.set("server.token", &crate::util::new_token());
        secrets.save(paths)?;
        println!("   Generated an API token (stored in {} with mode 600).", paths.secrets().display());
    } else {
        println!("   API token already exists.");
    }
    println!("   Show it any time with `alice token` (used to log in from the Web GUI and the iPhone app).");

    println!("\n4. Model provider");
    if cfg.providers.is_empty() || confirm("   Add a model provider now?", cfg.providers.is_empty()) {
        model_add(paths).await?;
    }

    println!("\n5. Service");
    if cfg!(any(target_os = "macos", target_os = "linux")) && confirm("   Register Alice to start automatically (launchd/systemd)?", true) {
        match service::register(paths) {
            Ok(m) => println!("   {m}"),
            Err(e) => println!("   could not register service: {e}"),
        }
    }
    println!("\nDone. Next: `alice start`, then `alice chat`.");
    Ok(())
}

// ---------------------------------------------------------------- status / token

async fn status(paths: &Paths) -> CliResult<()> {
    let c = Client::from_env(paths)?;
    let mgr = match service::manager() {
        service::Manager::Launchd => "launchd",
        service::Manager::Systemd => "systemd",
        service::Manager::None => "none",
    };
    if !c.is_up().await {
        println!("Alice is not running ({}; service: {mgr})", c.base);
        return Ok(());
    }
    let s = c.get("/status").await?;
    println!("Alice {} running at {}", s["version"].as_str().unwrap_or(""), c.base);
    println!("  pid {}  uptime {}s  memory {:.1} MB  cpu {}%", s["pid"], s["uptime_secs"], s["rss_kb"].as_f64().unwrap_or(0.0) / 1024.0, s["cpu_percent"]);
    println!("  threads {}  messages {}  memories {}  db {:.1} KB", s["counts"]["threads"], s["counts"]["messages"], s["counts"]["memories"], s["db_bytes"].as_f64().unwrap_or(0.0) / 1024.0);
    println!("  default model: {}", s["default_model"].as_str().filter(|m| !m.is_empty()).unwrap_or("(none)"));
    println!("  service: {mgr}  data: {}", s["home"].as_str().unwrap_or(""));
    Ok(())
}

async fn token_cmd(paths: &Paths, rotate: bool) -> CliResult<()> {
    let mut s = Secrets::load(paths)?;
    if rotate || s.get("server.token").is_none() {
        s.set("server.token", &crate::util::new_token());
        s.save(paths)?;
        println!("New token generated. Restart the server (`alice restart`) and log in again on other devices.");
    }
    let cfg = Config::load(paths)?;
    let url = if cfg.server.public_url.is_empty() { format!("http://{}:{}", cfg.server.bind, cfg.server.port) } else { cfg.server.public_url.clone() };
    println!("Server URL: {url}");
    println!("API token:  {}", s.get("server.token").unwrap_or(""));
    Ok(())
}

// ---------------------------------------------------------------- chat

fn looks_like_id(s: &str) -> bool {
    (4..=12).contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit() || (c.is_ascii_lowercase() && !"ilou".contains(c)))
}

async fn chat_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let c = Client::from_env(paths)?;
    let first = args.first().map(String::as_str);
    match (first, args.len()) {
        (None, _) => chat::interactive(&c, None).await,
        (Some("new"), 1) => chat::interactive(&c, None).await,
        (Some("list" | "ls"), 1) => chat::list(&c).await,
        (Some("delete" | "rm"), 2) => {
            c.delete(&format!("/threads/{}", args[1]), None).await?;
            println!("Deleted thread {} and the memories derived from it.", args[1]);
            Ok(())
        }
        (Some("rename"), n) if n >= 2 => {
            let title = if n > 2 { args[2..].join(" ") } else { ask("New title", "") };
            c.patch(&format!("/threads/{}", args[1]), &json!({ "title": title })).await?;
            Ok(())
        }
        (Some("clear"), 1) => {
            println!("This deletes ALL threads and every memory Alice learned from them.");
            println!("Settings, models, API keys and integrations are kept.");
            if ask("Type 'delete-all' to confirm", "") != "delete-all" {
                println!("Cancelled.");
                return Ok(());
            }
            c.delete("/threads", Some(&json!({ "confirm": "delete-all" }))).await?;
            println!("All threads and conversation-derived memories were deleted.");
            Ok(())
        }
        (Some(id), 1) if looks_like_id(id) => match c.get(&format!("/threads/{id}")).await {
            Ok(v) => chat::interactive(&c, v.pointer("/thread/id").and_then(|i| i.as_str()).map(str::to_string)).await,
            Err(_) => chat::one_shot(&c, id).await,
        },
        _ => chat::one_shot(&c, &args.join(" ")).await,
    }
}

// ---------------------------------------------------------------- model

fn model_hint(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "claude-opus-5-5",
        ProviderKind::CodexCli | ProviderKind::GeminiCli => "default",
        _ => "",
    }
}

async fn model_add(paths: &Paths) -> CliResult<()> {
    let kinds: Vec<&str> = ProviderKind::ALL.iter().map(|k| k.as_str()).collect();
    println!("Provider type:");
    let kind = ProviderKind::ALL[choose("Select", &kinds, 0)];
    match kind {
        ProviderKind::CodexCli => println!("Uses your ChatGPT subscription through the official Codex CLI (`codex login`). Text only: Alice tools are not available with this provider."),
        ProviderKind::GeminiCli => println!("Experimental: uses the official Gemini CLI login. If it fails, use the Gemini API (API key) instead. Text only."),
        _ => {}
    }
    let id = ask("Provider id", kind.as_str());
    let base_default = kind.default_base_url();
    let base_url = match kind {
        ProviderKind::OpenaiCompatible => ask("Base URL (e.g. http://host:8000/v1)", ""),
        ProviderKind::CodexCli | ProviderKind::GeminiCli => ask("Executable", base_default),
        _ => ask("Base URL", base_default),
    };
    let base_url = if base_url == base_default { String::new() } else { base_url };
    let models_raw = ask("Model name(s), comma separated", model_hint(kind));
    let models: Vec<String> = models_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if models.is_empty() {
        return Err("at least one model name is required".into());
    }
    let api_key = match kind {
        ProviderKind::Openai | ProviderKind::Anthropic | ProviderKind::Gemini => Some(ask_secret("API key (input hidden)")),
        ProviderKind::OpenaiCompatible => Some(ask_secret("API key (optional, input hidden)")).filter(|k| !k.is_empty()),
        _ => None,
    };
    let api_key = api_key.filter(|k| !k.is_empty());
    let default = match backend(paths).await {
        Backend::Remote(c) => {
            let v = c
                .post("/providers", &json!({ "id": id, "kind": kind.as_str(), "base_url": base_url, "models": models, "api_key": api_key }))
                .await?;
            v["default"].as_str().unwrap_or("").to_string()
        }
        Backend::Local => local_add_provider(
            paths,
            ProviderConfig { id: id.clone(), kind, base_url, models, context_tokens: 0 },
            api_key.as_deref(),
        )?,
    };
    println!("Added provider '{id}'. Default model: {default}");
    if kind == ProviderKind::CodexCli && confirm("Run `codex login` now?", false) {
        let _ = std::process::Command::new("codex").arg("login").status();
    }
    Ok(())
}

async fn models_json(paths: &Paths) -> CliResult<Value> {
    match backend(paths).await {
        Backend::Remote(c) => c.get("/models").await,
        Backend::Local => {
            let cfg = Config::load(paths)?;
            let s = Secrets::load(paths)?;
            let providers: Vec<Value> = cfg
                .providers
                .iter()
                .map(|p| json!({ "id": p.id, "kind": p.kind.as_str(), "models": p.models, "has_key": s.provider_key(&p.id).is_some() }))
                .collect();
            Ok(json!({ "default": cfg.agent.default_model, "models": cfg.all_models(), "providers": providers }))
        }
    }
}

fn print_models(v: &Value) -> Vec<String> {
    let default = v["default"].as_str().unwrap_or("");
    let mut all = Vec::new();
    for p in v["providers"].as_array().cloned().unwrap_or_default() {
        let kind = p["kind"].as_str().unwrap_or("");
        let auth = match ProviderKind::parse(kind) {
            Some(k) if k.needs_api_key() => if p["has_key"] == true { "key set" } else { "no key" },
            Some(ProviderKind::CodexCli | ProviderKind::GeminiCli) => "cli login",
            _ => "",
        };
        println!("{} {}", p["id"].as_str().unwrap_or(""), dim(&format!("({kind}{})", if auth.is_empty() { String::new() } else { format!(", {auth}") })));
        for m in p["models"].as_array().cloned().unwrap_or_default() {
            let full = format!("{}/{}", p["id"].as_str().unwrap_or(""), m.as_str().unwrap_or(""));
            all.push(full.clone());
            let mark = if full == default { "*" } else { " " };
            println!("  {mark} {:>2}) {full}", all.len());
        }
    }
    if all.is_empty() {
        println!("No models configured. Add one with `alice model add`.");
    }
    all
}

async fn set_default(paths: &Paths, model: &str) -> CliResult<()> {
    match backend(paths).await {
        Backend::Remote(c) => {
            c.put("/models/default", &json!({ "model": model })).await?;
        }
        Backend::Local => {
            let mut cfg = Config::load(paths)?;
            if cfg.resolve_model(model).is_none() {
                return Err(format!("unknown model: {model} (see `alice model list`)"));
            }
            cfg.agent.default_model = model.to_string();
            cfg.save(paths)?;
        }
    }
    println!("Default model: {model}");
    Ok(())
}

async fn model_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] => loop {
            let v = models_json(paths).await?;
            let all = print_models(&v);
            let ans = ask("\nNumber = set default, a = add, r <n> = remove, t <n> = test, Enter = quit", "");
            let (cmd, rest) = ans.split_once(' ').map(|(x, y)| (x, y.trim())).unwrap_or((ans.as_str(), ""));
            let pick = |s: &str| s.parse::<usize>().ok().and_then(|n| all.get(n.wrapping_sub(1)).cloned());
            match cmd {
                "" | "q" => return Ok(()),
                "a" => model_add(paths).await?,
                "r" => match pick(rest) {
                    Some(m) => remove_provider(paths, m.split('/').next().unwrap_or("")).await?,
                    None => println!("give a number"),
                },
                "t" => match pick(rest) {
                    Some(m) => test_model(paths, &m).await?,
                    None => println!("give a number"),
                },
                n => match pick(n) {
                    Some(m) => set_default(paths, &m).await?,
                    None => println!("unknown choice"),
                },
            }
            println!();
        },
        ["list" | "ls"] => {
            print_models(&models_json(paths).await?);
            Ok(())
        }
        ["add"] => model_add(paths).await,
        ["remove" | "rm", id] => remove_provider(paths, id.split('/').next().unwrap_or(id)).await,
        ["use", id] => set_default(paths, id).await,
        ["test", id] => test_model(paths, id).await,
        ["login", provider] => login(paths, provider).await,
        ["logout", provider] => logout(paths, provider).await,
        _ => Err("usage: alice model [list|add|remove <id>|use <id>|login <provider>|logout <provider>|test <id>]".into()),
    }
}

async fn remove_provider(paths: &Paths, id: &str) -> CliResult<()> {
    match backend(paths).await {
        Backend::Remote(c) => {
            c.delete(&format!("/providers/{id}"), None).await?;
        }
        Backend::Local => {
            let mut cfg = Config::load(paths)?;
            let before = cfg.providers.len();
            cfg.providers.retain(|p| p.id != id);
            if before == cfg.providers.len() {
                return Err(format!("no such provider: {id}"));
            }
            if cfg.resolve_model(&cfg.agent.default_model).is_none() {
                cfg.agent.default_model = cfg.all_models().into_iter().next().unwrap_or_default();
            }
            cfg.save(paths)?;
            let mut s = Secrets::load(paths)?;
            s.remove_prefix(&format!("provider.{id}."));
            s.save(paths)?;
        }
    }
    println!("Removed provider '{id}' and its API key.");
    Ok(())
}

async fn test_model(paths: &Paths, model: &str) -> CliResult<()> {
    let c = remote(paths).await?;
    println!("Testing {model} …");
    let v = c.post("/models/test", &json!({ "model": model })).await?;
    if v["ok"] == true {
        println!(
            "ok — reply {:?}, first token {} ms, total {} ms",
            v["reply"].as_str().unwrap_or(""),
            v["ttft_ms"].as_u64().map(|n| n.to_string()).unwrap_or("-".into()),
            v["total_ms"]
        );
        Ok(())
    } else {
        Err(format!("failed: {}", v["error"].as_str().unwrap_or("unknown error")))
    }
}

fn provider_kind(paths: &Paths, id: &str) -> CliResult<ProviderKind> {
    Config::load(paths)?.provider(id).map(|p| p.kind).ok_or_else(|| format!("no such provider: {id}"))
}

async fn set_secret(paths: &Paths, name: &str, value: &str) -> CliResult<()> {
    match backend(paths).await {
        Backend::Remote(c) => {
            if value.is_empty() {
                c.delete(&format!("/secrets/{name}"), None).await?;
            } else {
                c.put(&format!("/secrets/{name}"), &json!({ "value": value })).await?;
            }
        }
        Backend::Local => {
            let mut s = Secrets::load(paths)?;
            s.set(name, value);
            s.save(paths)?;
        }
    }
    Ok(())
}

async fn login(paths: &Paths, provider: &str) -> CliResult<()> {
    match provider_kind(paths, provider)? {
        ProviderKind::CodexCli => {
            println!("Opening the official Codex login (Sign in with ChatGPT)…");
            let st = std::process::Command::new("codex").arg("login").status().map_err(|e| format!("codex: {e}"))?;
            if st.success() { Ok(()) } else { Err("codex login failed".into()) }
        }
        ProviderKind::GeminiCli => {
            println!("Run `gemini` once in a terminal and choose \"Login with Google\". Alice reuses that login.");
            Ok(())
        }
        ProviderKind::Ollama => {
            println!("Ollama needs no login.");
            Ok(())
        }
        _ => {
            let key = ask_secret("API key (input hidden)");
            if key.is_empty() {
                return Err("no key given".into());
            }
            set_secret(paths, &format!("provider.{provider}.api_key"), &key).await?;
            println!("Saved API key for '{provider}'.");
            Ok(())
        }
    }
}

async fn logout(paths: &Paths, provider: &str) -> CliResult<()> {
    match provider_kind(paths, provider)? {
        ProviderKind::CodexCli => {
            let _ = std::process::Command::new("codex").arg("logout").status();
            Ok(())
        }
        ProviderKind::GeminiCli => {
            println!("Sign out inside the Gemini CLI (`gemini`, then /auth).");
            Ok(())
        }
        _ => {
            set_secret(paths, &format!("provider.{provider}.api_key"), "").await?;
            println!("Removed API key for '{provider}'.");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- memory

fn print_memories(v: &Value) {
    let list = v["memories"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        println!("No memories.");
    }
    for m in list {
        println!(
            "{:>4}  {}  {}",
            m["id"],
            m["content"].as_str().unwrap_or(""),
            dim(&format!("[{} · {} source(s)]", m["key"].as_str().unwrap_or(""), m["source_count"]))
        );
    }
}

async fn memory_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let c = Client::from_env(paths)?;
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] | ["list" | "ls"] => {
            print_memories(&c.get("/memories?limit=200").await?);
            Ok(())
        }
        ["search", rest @ ..] if !rest.is_empty() => {
            let q = rest.join(" ");
            let url = format!("/memories?q={}", urlencode(&q));
            print_memories(&c.get(&url).await?);
            Ok(())
        }
        ["show", id] => {
            let v = c.get(&format!("/memories/{id}")).await?;
            println!("{}", v["memory"]["content"].as_str().unwrap_or(""));
            println!("{}", dim(&format!("key: {}", v["memory"]["key"].as_str().unwrap_or(""))));
            println!("Sources:");
            for s in v["sources"].as_array().cloned().unwrap_or_default() {
                println!("  thread {} ({}) · message {}", s["thread_id"].as_str().unwrap_or(""), s["thread_title"].as_str().unwrap_or(""), s["message_id"].as_str().unwrap_or(""));
                println!("    {}", dim(s["content"].as_str().unwrap_or("")));
            }
            Ok(())
        }
        ["status"] => {
            let v = c.get("/memories/status").await?;
            let s = &v["status"];
            println!("memories {}  sources {}  threads {}", s["memories"], s["sources"], s["threads"]);
            println!("database {:.1} KB  attachments {:.1} KB", s["db_bytes"].as_f64().unwrap_or(0.0) / 1024.0, s["attachments_bytes"].as_f64().unwrap_or(0.0) / 1024.0);
            println!("auto extraction: {}  injected per turn: {}", v["auto_extract"], v["inject_limit"]);
            Ok(())
        }
        ["clear"] => {
            println!("This deletes all memories. Conversation threads are kept; old messages are not re-learned.");
            if !confirm("Continue?", false) {
                return Ok(());
            }
            c.delete("/memories", Some(&json!({ "confirm": "clear-memories" }))).await?;
            println!("All memories deleted.");
            Ok(())
        }
        _ => Err("usage: alice memory [list|search \"keyword\"|show <id>|status|clear]".into()),
    }
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------- config / tools

async fn config_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] => {
            let cfg = match backend(paths).await {
                Backend::Remote(c) => {
                    let v = c.get("/config").await?;
                    serde_json::from_value::<Config>(v).map_err(|e| e.to_string())?
                }
                Backend::Local => Config::load(paths)?,
            };
            print!("{}", toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?);
            println!("{}", dim(&format!("# file: {} (credentials are kept separately in secrets.toml)", paths.config().display())));
            Ok(())
        }
        ["get", key] => {
            let cfg = match backend(paths).await {
                Backend::Remote(c) => serde_json::from_value::<Config>(c.get("/config").await?).map_err(|e| e.to_string())?,
                Backend::Local => Config::load(paths)?,
            };
            println!("{}", cfg.get_key(key).ok_or_else(|| format!("unknown key: {key}"))?);
            Ok(())
        }
        ["set", key, value @ ..] if !value.is_empty() => {
            let value = value.join(" ");
            match backend(paths).await {
                Backend::Remote(c) => {
                    let mut cfg = serde_json::from_value::<Config>(c.get("/config").await?).map_err(|e| e.to_string())?;
                    cfg.set_key(key, &value)?; // validate locally first
                    let v = cfg_value(&cfg, key).ok_or_else(|| format!("unknown key: {key}"))?;
                    let body = json!({ *key: serde_json::to_value(v).map_err(|e| e.to_string())? });
                    c.patch("/config", &body).await?;
                }
                Backend::Local => {
                    let mut cfg = Config::load(paths)?;
                    cfg.set_key(key, &value)?;
                    cfg.save(paths)?;
                }
            }
            println!("{key} = {value}");
            if key.starts_with("server.") {
                println!("{}", dim("restart the server to apply (`alice restart`)"));
            }
            Ok(())
        }
        ["secret", name] => {
            let value = ask_secret(&format!("Value for {name} (input hidden, empty = remove)"));
            set_secret(paths, name, &value).await?;
            println!("{name} {}", if value.is_empty() { "removed" } else { "saved" });
            Ok(())
        }
        _ => Err("usage: alice config [get <key> | set <key> <value> | secret <name>]\n  secrets: brave.api_key, telegram.bot_token, discord.bot_token, google.client_secret, provider.<id>.api_key".into()),
    }
}

fn cfg_value(cfg: &Config, key: &str) -> Option<toml::Value> {
    toml::Value::try_from(cfg).ok().and_then(|root| key.split('.').try_fold(root, |cur, part| cur.get(part).cloned()))
}

async fn tool_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let set = |name: &str, perm: Option<Permission>| {
        let name = name.to_string();
        async move {
            match backend(paths).await {
                Backend::Remote(c) => {
                    c.patch("/tools", &json!({ &name: perm.map(|p| p.as_str()) })).await?;
                }
                Backend::Local => {
                    if crate::tools::find(&name).is_none() && !["fs", "web", "google"].contains(&name.as_str()) {
                        return Err(format!("unknown tool: {name}"));
                    }
                    let mut cfg = Config::load(paths)?;
                    match perm {
                        Some(p) => {
                            cfg.tools.permissions.insert(name.clone(), p);
                        }
                        None => {
                            cfg.tools.permissions.remove(&name);
                        }
                    }
                    cfg.save(paths)?;
                }
            }
            Ok::<(), String>(())
        }
    };
    match a.as_slice() {
        [] | ["list" | "ls"] => {
            let cfg = match backend(paths).await {
                Backend::Remote(c) => serde_json::from_value::<Config>(c.get("/config").await?).map_err(|e| e.to_string())?,
                Backend::Local => Config::load(paths)?,
            };
            for d in crate::tools::all() {
                let p = crate::tools::permission(&cfg, d);
                println!("{:<18} {:<9} {}", d.name, p.as_str(), dim(d.description));
            }
            let roots = if cfg.tools.fs.roots.is_empty() { vec![paths.workspace().display().to_string()] } else { cfg.tools.fs.roots.clone() };
            println!("{}", dim(&format!("file system roots: {}", roots.join(", "))));
            Ok(())
        }
        ["enable", name] => {
            set(name, Some(Permission::Allow)).await?;
            println!("{name}: allow");
            Ok(())
        }
        ["enable", name, mode] => {
            let p = Permission::parse(mode).filter(|p| *p != Permission::Disabled).ok_or("mode must be ask or allow")?;
            set(name, Some(p)).await?;
            println!("{name}: {}", p.as_str());
            Ok(())
        }
        ["disable", name] => {
            set(name, Some(Permission::Disabled)).await?;
            println!("{name}: disabled");
            Ok(())
        }
        ["reset", name] => {
            set(name, None).await?;
            println!("{name}: default");
            Ok(())
        }
        _ => Err("usage: alice tool [list | enable <tool> [ask|allow] | disable <tool> | reset <tool>]".into()),
    }
}
