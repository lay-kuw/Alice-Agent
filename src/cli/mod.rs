//! `alice` command line. Everything except `serve` is a client of the
//! server API; settings commands edit the config files directly only when
//! the server is not running (so `alice setup` / `alice model` work first).

pub mod chat;
pub mod client;
pub mod markdown;
pub mod profile;
pub mod service;
pub mod tui;
pub mod ui;

use crate::config::{Config, Paths, Permission, ProviderConfig, ProviderKind, Secrets};
use chat::{dim, green, red, yellow};
use clap::{Parser, Subcommand};
use client::{CliResult, Client};
use serde_json::{Value, json};
use std::io::Write;

#[derive(Parser)]
#[command(name = "alice", version, about = "Alice Agent (A2) — a lightweight personal AI agent", disable_help_subcommand = true)]
pub struct Cli {
    /// Use this profile instead of the current one
    #[arg(short = 'p', long, global = true)]
    pub profile: Option<String>,
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
    /// Chat: `alice chat` (TUI), `alice chat "message"`, `alice chat new|list|<id>|delete <id>|rename <id>|clear`, `--plain`
    Chat { args: Vec<String> },
    /// Manage models: list, add, remove <id>, use <id>, login <provider>, logout <provider>, test <id>
    Model {
        /// Options after the provider go to its own login (e.g. `--device-auth`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Memory: list, search "keyword", status, clear
    Memory { args: Vec<String> },
    /// Config: `alice config`, `get <key>`, `set <key> <value>`, `secret <name>`
    Config { args: Vec<String> },
    /// Tools: list, enable <tool> [ask|allow], disable <tool>
    Tool { args: Vec<String> },
    /// Telegram / Discord gateways: setup, list, delete <gw-id>
    Gateway { args: Vec<String> },
    /// Profiles: `alice profile` (list), `alice profile <name>` (create/switch), `use <name>`, `delete <name>`
    Profile { args: Vec<String> },
    /// Subscription limits (Codex) and tokens used
    Usage,
    /// Personality (SOUL.md): edit, show, reset
    Soul { args: Vec<String> },
    /// Skills: list, info <id>, enable/disable <id>, setup <id>, login/logout <id>, log
    Skill { args: Vec<String> },
    /// Shortcut for the google-workspace skill: status, setup, login, logout
    Google { args: Vec<String> },
    /// Auto-start service: install, uninstall, status
    Service { args: Vec<String> },
    /// Show the API token and connection info for Web / iPhone (`--rotate` for a new random one, `--set` to choose your own)
    Token {
        #[arg(long)]
        rotate: bool,
        /// Type your own token (asked with hidden input)
        #[arg(long, conflicts_with = "rotate")]
        set: bool,
    },
    /// Run the server in the foreground (used by the service manager)
    #[command(hide = true)]
    Serve,
    /// Local stdio MCP server, started by Alice for Codex (one turn at a time)
    #[command(hide = true)]
    Mcp,
}

pub async fn run(cmd: Option<Cmd>, paths: Paths) -> CliResult<()> {
    crate::providers::cli::set_codex_home(paths.codex_home());
    match cmd {
        None => {
            if !paths.config().exists() && paths.is_default() {
                println!("Welcome to Alice. Run `alice setup` to get started, or `alice --help`.");
                return Ok(());
            }
            let c = Client::from_env(&paths)?;
            interactive(&c, None, &paths, false).await
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
        Some(Cmd::Token { rotate, set }) => token_cmd(&paths, rotate, set).await,
        Some(Cmd::Profile { args }) => profile::cmd(&paths, args).await,
        Some(Cmd::Service { args }) => service_cmd(&paths, args),
        Some(Cmd::Google { args }) => google_cmd(&paths, args).await,
        Some(Cmd::Skill { args }) => skill_cmd(&paths, args).await,
        Some(Cmd::Soul { args }) => soul_cmd(&paths, args),
        Some(Cmd::Usage) => {
            let c = Client::from_env(&paths)?;
            print!("{}", crate::usage::render_text(&c.get("/usage").await?));
            Ok(())
        }
        Some(Cmd::Serve) => unreachable!(),
        Some(Cmd::Mcp) => crate::mcp::serve_stdio().await,
    }
}

// ---------------------------------------------------------------- prompts

pub fn ask(prompt: &str, default: &str) -> String {
    print!("{}", ui::question(prompt, default));
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    let s = s.trim();
    if s.is_empty() { default.to_string() } else { s.to_string() }
}

pub fn ask_secret(prompt: &str) -> String {
    print!("{}{}", ui::question(prompt, ""), ui::dim("(hidden) "));
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
    let a = ask(&format!("{} {}", prompt.trim(), ui::dim(if default { "(Y/n)" } else { "(y/N)" })), "");
    match a.to_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        _ => false,
    }
}

/// Numbered options, each with a short note.
fn choose_with(prompt: &str, options: &[(&str, &str)], default: usize) -> usize {
    use unicode_width::UnicodeWidthStr;
    let w = options.iter().map(|(l, _)| l.width()).max().unwrap_or(0);
    for (i, (label, note)) in options.iter().enumerate() {
        ui::option(i + 1, label, note, w, i == default);
    }
    loop {
        let a = ask(prompt, &(default + 1).to_string());
        if let Ok(n) = a.parse::<usize>()
            && n >= 1 && n <= options.len() {
                return n - 1;
            }
        ui::warn(&format!("pick a number from 1 to {}", options.len()));
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
    const STEPS: usize = 5;
    ui::card("✦ Alice Agent · setup", &[format!("profile {} · {}", paths.profile, ui::path(&paths.home)), "Enter keeps the value in [brackets] ♡".into()]);
    paths.ensure().map_err(|e| e.to_string())?;
    let mut cfg = Config::load(paths)?;

    ui::step(1, STEPS, "Server");
    ui::note("127.0.0.1 is always on. Add this machine's LAN or Tailscale IP to reach Alice\nfrom other devices (comma-separated; 0.0.0.0 inside a container).\nPlain HTTP: use trusted networks only.");
    cfg.server.bind = ask("Listen addresses", &cfg.server.bind);
    loop {
        let p = ask("Port", &cfg.server.port.to_string());
        if let Ok(n) = p.parse::<u16>() {
            cfg.server.port = n;
            break;
        }
        ui::warn("a port is a number from 1 to 65535");
    }
    cfg.server.public_url = ask("Public URL for other devices (optional)", &cfg.server.public_url);
    cfg.agent.language = ask("Reply language", &cfg.agent.language);
    cfg.save(paths)?;
    ui::ok(&format!("saved {}", ui::path(&paths.config())));

    ui::step(2, STEPS, "Sign-in token");
    let mut secrets = Secrets::load(paths)?;
    if secrets.get("server.token").is_none() {
        secrets.set("server.token", &crate::util::new_token());
        secrets.save(paths)?;
        ui::ok("made a new API token (stored with mode 600)");
    } else {
        ui::ok("API token already set");
    }
    ui::note("Show it any time with `alice token`: the Web UI and the iPhone app sign in with it.");

    ui::step(3, STEPS, "Personality");
    crate::config::ensure_soul(paths);
    ui::ok(&ui::path(&paths.soul()));
    ui::note("Alice's voice and manners. Edit any time with `alice soul`.");

    ui::step(4, STEPS, "Model");
    if cfg.providers.is_empty() || confirm("Add a model provider now?", cfg.providers.is_empty()) {
        model_add(paths).await?;
    } else {
        ui::ok(&format!("default model: {}", cfg.agent.default_model));
    }

    ui::step(5, STEPS, "Start automatically");
    if cfg!(any(target_os = "macos", target_os = "linux")) && confirm("Start Alice at login (launchd / systemd)?", true) {
        match service::register(paths) {
            Ok(m) => ui::ok(&m),
            Err(e) => ui::warn(&format!("could not register the service: {e}")),
        }
    }
    ui::done("✦ All set! ₍ᐢ. .ᐢ₎♡", &[("alice start", "start the server"), ("alice chat", "talk with Alice"), ("alice token", "sign in from Web / iPhone")]);
    Ok(())
}

// ---------------------------------------------------------------- status / token

async fn status(paths: &Paths) -> CliResult<()> {
    let c = Client::from_env(paths)?;
    let mgr = match service::manager(paths) {
        service::Manager::Launchd => "launchd",
        service::Manager::Systemd => "systemd",
        service::Manager::None => "none",
    };
    if !c.is_up().await {
        println!("Alice is not running ({}; profile: {}; service: {mgr})", c.base, paths.profile);
        return Ok(());
    }
    let s = c.get("/status").await?;
    println!("Alice {} running at {} (profile: {})", s["version"].as_str().unwrap_or(""), c.base, paths.profile);
    println!("  pid {}  uptime {}s  memory {:.1} MB  cpu {}%", s["pid"], s["uptime_secs"], s["rss_kb"].as_f64().unwrap_or(0.0) / 1024.0, s["cpu_percent"]);
    println!("  threads {}  messages {}  memories {}  db {:.1} KB", s["counts"]["threads"], s["counts"]["messages"], s["counts"]["memories"], s["db_bytes"].as_f64().unwrap_or(0.0) / 1024.0);
    println!("  default model: {}", s["default_model"].as_str().filter(|m| !m.is_empty()).unwrap_or("(none)"));
    println!("  service: {mgr}  data: {}", s["home"].as_str().unwrap_or(""));
    Ok(())
}

async fn token_cmd(paths: &Paths, rotate: bool, set: bool) -> CliResult<()> {
    let mut s = Secrets::load(paths)?;
    if set {
        let t = ask_secret("New API token (input hidden)");
        crate::util::check_token(&t)?;
        if ask_secret("Again") != t {
            return Err("the two entries differ; nothing changed".into());
        }
        s.set("server.token", &t);
        s.save(paths)?;
        println!("Token saved. The running server uses it right away; log in again on the Web and iPhone.");
    } else if rotate || s.get("server.token").is_none() {
        s.set("server.token", &crate::util::new_token());
        s.save(paths)?;
        println!("New token generated. The running server uses it right away; log in again on the Web and iPhone.");
    }
    let cfg = Config::load(paths)?;
    println!("Profile:    {}", paths.profile);
    if !cfg.server.public_url.is_empty() {
        println!("Server URL: {}", cfg.server.public_url);
    } else {
        // Every address other devices can use, then this machine's own.
        let mut urls: Vec<String> = crate::server::listen_addrs(&cfg.server.bind)
            .into_iter()
            .filter(|a| !matches!(a.as_str(), "127.0.0.1" | "localhost" | "::1"))
            .map(|a| match a.as_str() {
                "0.0.0.0" | "::" => format!("http://<this machine's IP>:{}", cfg.server.port),
                h if h.contains(':') => format!("http://[{h}]:{}", cfg.server.port),
                h => format!("http://{h}:{}", cfg.server.port),
            })
            .collect();
        urls.push(format!("{} (this machine only)", crate::cli::client::local_url(&cfg)));
        for (i, u) in urls.iter().enumerate() {
            println!("{} {u}", if i == 0 { "Server URL:" } else { "           " });
        }
    }
    if !set {
        println!("API token:  {}", s.get("server.token").unwrap_or(""));
    }
    Ok(())
}

/// SOUL.md is a settings file, so it is edited directly (the server reads it every turn).
fn soul_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    paths.ensure().map_err(|e| e.to_string())?;
    crate::config::ensure_soul(paths);
    let file = paths.soul();
    match args.first().map(String::as_str) {
        None | Some("edit") => {
            let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("{editor} \"$1\""))
                .arg("sh")
                .arg(&file)
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("{editor} exited with {status}"));
            }
            println!("Saved {}. Alice uses it from the next reply.", file.display());
            Ok(())
        }
        Some("show") => {
            print!("{}", crate::config::read_soul(paths));
            Ok(())
        }
        Some("path") => {
            println!("{}", file.display());
            Ok(())
        }
        Some("reset") => {
            if !confirm("Replace SOUL.md with the default template?", false) {
                return Ok(());
            }
            crate::config::write_atomic(&file, crate::config::SOUL_TEMPLATE.as_bytes(), 0o600)?;
            println!("Reset {}.", file.display());
            Ok(())
        }
        _ => Err("usage: alice soul [edit | show | path | reset]".into()),
    }
}

/// `alice google …` is kept as a shortcut for the google-workspace skill.
async fn google_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let mut a = args;
    if a.is_empty() {
        a.push("info".into());
    }
    if a[0] == "status" {
        a[0] = "info".into();
    }
    a.insert(1, "google-workspace".into());
    skill_cmd(paths, a).await
}

fn find_skill<'a>(v: &'a Value, id: &str) -> CliResult<&'a Value> {
    v["skills"].as_array().and_then(|a| a.iter().find(|s| s["id"] == id)).ok_or_else(|| format!("unknown skill: {id} (alice skill list)"))
}

fn status_mark(s: &Value) -> String {
    match s["status"].as_str().unwrap_or("") {
        "ready" => green("✓ ready"),
        "disabled" => dim("○ off"),
        st => yellow(&format!("! {}", if s["detail"].as_str().unwrap_or("").is_empty() { st } else { s["detail"].as_str().unwrap_or("") })),
    }
}

async fn skill_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let c = remote(paths).await?;
    match a.as_slice() {
        [] | ["list" | "ls"] => {
            let v = c.get("/skills").await?;
            for s in v["skills"].as_array().into_iter().flatten() {
                let tools = s["tools"].as_array().map(|t| t.len()).unwrap_or(0);
                let src = if s["source"] == "builtin" { "built-in" } else { "user" };
                println!("{:<18} {:<26} {}", s["id"].as_str().unwrap_or(""), status_mark(s), dim(&format!("{tools} tools · {src} · {}", s["description"].as_str().unwrap_or(""))));
            }
            for i in v["invalid"].as_array().into_iter().flatten() {
                println!("{:<18} {}", i["folder"].as_str().unwrap_or(""), red(&format!("✗ {}", i["error"].as_str().unwrap_or(""))));
            }
            println!("{}", dim(&format!("skills folder: {}  ·  alice skill info <id>", v["dir"].as_str().unwrap_or(""))));
            Ok(())
        }
        ["info" | "show", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            println!("{} {}  {}", s["name"].as_str().unwrap_or(""), dim(s["version"].as_str().unwrap_or("")), status_mark(s));
            println!("{}", s["description"].as_str().unwrap_or(""));
            println!("{}", dim(&format!("source: {}", s["source"].as_str().unwrap_or(""))));
            let p = &s["permissions"];
            let net: Vec<&str> = p["network"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
            println!("\nDeclared permissions");
            println!("  network: {}", if net.is_empty() { "none".to_string() } else { net.join(", ") });
            println!("  files:   {}", if p["files"] == true { "yes" } else { "no" });
            if let Some(d) = p["description"].as_str().filter(|d| !d.is_empty()) {
                println!("  {}", dim(d));
            }
            if let Some(au) = s["auth"].as_object() {
                println!("\nAuth: {} — {}", au["label"].as_str().unwrap_or(""), if au["connected"] == true { green("connected") } else { yellow("not connected (alice skill login …)") });
            }
            let settings: Vec<&Value> = s["settings"].as_array().into_iter().flatten().filter(|x| x["advanced"] != true).collect();
            if !settings.is_empty() {
                println!("\nSettings");
                for x in settings {
                    let val = if x["secret"] == true { if x["set"] == true { "••••".to_string() } else { "(not set)".into() } } else { x["value"].as_str().filter(|v| !v.is_empty()).unwrap_or("(not set)").to_string() };
                    println!("  {:<16} {}  {}", x["key"].as_str().unwrap_or(""), val, dim(x["label"].as_str().unwrap_or("")));
                }
            }
            if let Some(g) = s["groups"].as_array().filter(|g| !g.is_empty()) {
                let on: Vec<String> = g.iter().map(|x| format!("{}{}", if x["enabled"] == true { "✓" } else { "○" }, x["id"].as_str().unwrap_or(""))).collect();
                println!("\nGroups: {}", on.join("  "));
            }
            println!("\nTools");
            for t in s["tools"].as_array().into_iter().flatten() {
                let avail = if t["available"] == true { String::new() } else { dim(&format!(" ({})", t["reason"].as_str().unwrap_or(""))) };
                println!("  {:<20} {:<9} {}{avail}", t["name"].as_str().unwrap_or(""), t["permission"].as_str().unwrap_or(""), dim(t["description"].as_str().unwrap_or("")));
            }
            Ok(())
        }
        ["enable", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            if s["source"] != "builtin" {
                let net: Vec<&str> = s["permissions"]["network"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
                println!("{} declares:", s["name"].as_str().unwrap_or(id));
                println!("  network: {}", if net.is_empty() { "none".to_string() } else { net.join(", ") });
                println!("  files:   {}", if s["permissions"]["files"] == true { "yes" } else { "no" });
                let cmd = s["tools"].as_array().into_iter().flatten().any(|t| t["handler"] == "command");
                if cmd {
                    println!("  {}", yellow("runs local programs from its folder"));
                }
                if !ask("Enable? [y/N]", "n").eq_ignore_ascii_case("y") {
                    return Ok(());
                }
            }
            c.patch(&format!("/skills/{id}"), &json!({ "enabled": true })).await?;
            println!("{id}: enabled");
            Ok(())
        }
        ["disable", id] => {
            c.patch(&format!("/skills/{id}"), &json!({ "enabled": false })).await?;
            println!("{id}: disabled");
            Ok(())
        }
        ["set", id, key, value @ ..] => {
            c.patch(&format!("/skills/{id}"), &json!({ "settings": { *key: value.join(" ") } })).await?;
            println!("{id}.{key} saved");
            Ok(())
        }
        ["groups", id, list] => {
            let groups: Vec<&str> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
            c.patch(&format!("/skills/{id}"), &json!({ "groups": groups })).await?;
            println!("{id}: {}", groups.join(", "));
            Ok(())
        }
        ["setup", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            if *id == "google-workspace" {
                println!("Create an OAuth client of type \"Desktop app\" in Google Cloud Console (APIs & Services → Credentials),");
                println!("and enable the Gmail, Calendar, Drive, Docs and Sheets APIs you want to use.");
                println!("Note: while the consent screen is in \"Testing\", Google expires the login after 7 days.\n");
            }
            let mut settings = serde_json::Map::new();
            for x in s["settings"].as_array().into_iter().flatten().filter(|x| x["advanced"] != true) {
                let key = x["key"].as_str().unwrap_or("");
                let label = x["label"].as_str().filter(|l| !l.is_empty()).unwrap_or(key);
                let val = if x["secret"] == true {
                    ask_secret(&format!("{label} (input hidden; Enter to keep)"))
                } else {
                    ask(label, x["value"].as_str().unwrap_or(""))
                };
                if !val.is_empty() {
                    settings.insert(key.to_string(), json!(val));
                }
            }
            let mut body = json!({ "settings": settings, "enabled": true });
            if let Some(g) = s["groups"].as_array().filter(|g| !g.is_empty()) {
                let all: Vec<&str> = g.iter().filter_map(|x| x["id"].as_str()).collect();
                let cur: Vec<&str> = g.iter().filter(|x| x["enabled"] == true).filter_map(|x| x["id"].as_str()).collect();
                let pick = ask(&format!("Groups ({})", all.join(",")), &cur.join(","));
                body["groups"] = json!(pick.split(',').map(str::trim).filter(|x| all.contains(x)).collect::<Vec<_>>());
            }
            c.patch(&format!("/skills/{id}"), &body).await?;
            println!("Saved.{}", if s["auth"].is_object() { format!(" Now run `alice skill login {id}`.") } else { String::new() });
            Ok(())
        }
        ["login", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            match s["auth"]["type"].as_str() {
                Some("token") => {
                    let t = ask_secret(&format!("{} (input hidden)", s["auth"]["label"].as_str().unwrap_or("Token")));
                    c.post(&format!("/skills/{id}/login"), &json!({ "token": t })).await?;
                    println!("{id}: connected");
                    Ok(())
                }
                Some("oauth2") => {
                    let r = c.post(&format!("/skills/{id}/login"), &json!({})).await?;
                    let url = r["auth_url"].as_str().unwrap_or("");
                    println!("Open this URL in a browser on this machine and approve access:\n\n{url}\n");
                    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
                    let _ = std::process::Command::new(opener).arg(url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
                    println!("Waiting… If the browser is on another machine, its final page will fail to load;");
                    println!("copy that page's full URL from the address bar and paste it here (or just wait).");
                    let mut input = chat::Input::new();
                    for _ in 0..600 {
                        tokio::select! {
                            line = input.line("") => {
                                let line = line.unwrap_or_default();
                                if line.contains("code=") {
                                    c.post("/skills/finish", &json!({ "url": line })).await?;
                                }
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                        }
                        let v = c.get("/skills").await?;
                        if find_skill(&v, id)?["auth"]["connected"] == true {
                            println!("{id}: connected");
                            return Ok(());
                        }
                    }
                    Err("timed out waiting for the login".into())
                }
                _ => Err(format!("{id} has no login")),
            }
        }
        ["logout", id] => {
            c.post(&format!("/skills/{id}/logout"), &json!({})).await?;
            println!("{id}: disconnected (credentials removed)");
            Ok(())
        }
        ["rescan"] => {
            c.post("/skills/rescan", &json!({})).await?;
            Box::pin(skill_cmd(paths, vec![])).await
        }
        ["log"] | ["log", _] => {
            let n = a.get(1).and_then(|n| n.parse::<usize>().ok()).unwrap_or(30);
            let v = c.get(&format!("/tool-runs?limit={n}")).await?;
            for r in v["runs"].as_array().into_iter().flatten().rev() {
                let st = r["status"].as_str().unwrap_or("");
                let st = match st {
                    "ok" => green(st),
                    "error" | "denied" | "unavailable" => red(st),
                    _ => yellow(st),
                };
                println!(
                    "{}  {:<20} {:<16} {:<6} {:<12} {:>6}ms {}",
                    dim(&crate::util::fmt_utc(r["ts"].as_i64().unwrap_or(0))[5..16]),
                    r["tool"].as_str().unwrap_or(""),
                    r["skill"].as_str().unwrap_or(""),
                    r["source"].as_str().unwrap_or(""),
                    st,
                    r["duration_ms"],
                    dim(r["error"].as_str().unwrap_or(""))
                );
            }
            Ok(())
        }
        _ => Err("usage: alice skill [list | info <id> | enable <id> | disable <id> | setup <id> | set <id> <key> <value> | groups <id> a,b | login <id> | logout <id> | log [n] | rescan]".into()),
    }
}

fn service_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    match args.first().map(String::as_str) {
        Some("install") => {
            println!("{}", service::register(paths)?);
            Ok(())
        }
        Some("uninstall") => {
            service::unregister(paths)?;
            println!("Service for profile '{}' removed.", paths.profile);
            Ok(())
        }
        None | Some("status") => {
            let m = match service::manager(paths) {
                service::Manager::Launchd => format!("launchd ({})", service::launchd_plist(paths).display()),
                service::Manager::Systemd => format!("systemd ({})", service::systemd_unit(paths).display()),
                service::Manager::None => "not registered".into(),
            };
            println!("profile {}: {m}", paths.profile);
            Ok(())
        }
        _ => Err("usage: alice service [install | uninstall | status]".into()),
    }
}

// ---------------------------------------------------------------- chat

fn looks_like_id(s: &str) -> bool {
    (4..=12).contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit() || (c.is_ascii_lowercase() && !"ilou".contains(c)))
}

/// Full-screen TUI on a terminal; the line-based REPL otherwise (or with --plain).
async fn interactive(c: &Client, thread: Option<String>, paths: &Paths, plain: bool) -> CliResult<()> {
    use std::io::IsTerminal;
    if plain || !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() || std::env::var("TERM").as_deref() == Ok("dumb") {
        chat::interactive(c, thread).await
    } else {
        tui::run(c, thread, &paths.profile).await
    }
}

async fn chat_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let c = Client::from_env(paths)?;
    let plain = args.iter().any(|a| a == "--plain");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--plain").collect();
    let first = args.first().map(String::as_str);
    match (first, args.len()) {
        (None, _) => interactive(&c, None, paths, plain).await,
        (Some("new"), 1) => interactive(&c, None, paths, plain).await,
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
            Ok(v) => interactive(&c, v.pointer("/thread/id").and_then(|i| i.as_str()).map(str::to_string), paths, plain).await,
            Err(_) => chat::one_shot(&c, id).await,
        },
        _ => chat::one_shot(&c, &args.join(" ")).await,
    }
}

// ---------------------------------------------------------------- model

fn model_hint(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "claude-opus-5-5",
        ProviderKind::CodexCli | ProviderKind::GeminiCli | ProviderKind::AntigravityCli => "default",
        _ => "",
    }
}

/// One line about each provider type, for the picker.
fn kind_note(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Openai => "API key · GPT models",
        ProviderKind::Anthropic => "API key · Claude models",
        ProviderKind::Gemini => "API key · Gemini models",
        ProviderKind::OpenaiCompatible => "any OpenAI-style server (vLLM, LM Studio…)",
        ProviderKind::Ollama => "local models on this machine",
        ProviderKind::CodexCli => "ChatGPT subscription · official Codex CLI",
        ProviderKind::AntigravityCli => "Google AI subscription · Antigravity CLI",
        ProviderKind::GeminiCli => "retired",
    }
}

async fn model_add(paths: &Paths) -> CliResult<()> {
    ui::heading("Add a model provider");
    let rows: Vec<(&str, &str)> = ProviderKind::ADDABLE.iter().map(|k| (k.as_str(), kind_note(*k))).collect();
    let kind = ProviderKind::ADDABLE[choose_with("Provider", &rows, 0)];
    match kind {
        ProviderKind::CodexCli => ui::note("Uses your ChatGPT subscription through the official Codex CLI, with Alice's own login.\nSkills are reached through Alice's built-in MCP server."),
        ProviderKind::AntigravityCli => ui::note("Uses your Google AI subscription through the official Antigravity CLI (agy).\nSign in once with `alice model login <id>`. Text only; skills work through Alice's text protocol."),
        ProviderKind::Ollama => ui::note("Start Ollama first (`ollama serve`) and pull a model (`ollama pull <name>`)."),
        _ => {}
    }
    let id = ask("Provider id", kind.as_str());
    let base_default = kind.default_base_url();
    let base_url = match kind {
        ProviderKind::OpenaiCompatible => ask("Base URL (e.g. http://host:8000/v1)", ""),
        ProviderKind::CodexCli | ProviderKind::GeminiCli | ProviderKind::AntigravityCli => ask("Executable", base_default),
        _ => ask("Base URL", base_default),
    };
    let base_url = if base_url == base_default { String::new() } else { base_url };
    let models_raw = ask("Model name(s), comma separated", model_hint(kind));
    let models: Vec<String> = models_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if models.is_empty() {
        return Err("at least one model name is required".into());
    }
    let api_key = match kind {
        ProviderKind::Openai | ProviderKind::Anthropic | ProviderKind::Gemini => Some(ask_secret("API key")),
        ProviderKind::OpenaiCompatible => Some(ask_secret("API key (optional)")).filter(|k| !k.is_empty()),
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
    ui::ok(&format!("added {}  {}", ui::bold(&id), ui::dim(&format!("default model: {default}"))));
    if kind == ProviderKind::CodexCli && confirm("Log in to ChatGPT for Alice now?", true) {
        codex_login(paths, &[])?;
    }
    if kind == ProviderKind::AntigravityCli && confirm("Sign in to Antigravity now?", true) {
        agy_login(paths, &id)?;
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

/// Provider health from the running server (empty when it is not running).
async fn provider_status(paths: &Paths) -> std::collections::HashMap<String, (String, String)> {
    let mut m = std::collections::HashMap::new();
    if let Backend::Remote(c) = backend(paths).await
        && let Ok(v) = c.get("/providers/status").await {
            for p in v["providers"].as_array().into_iter().flatten() {
                m.insert(
                    p["id"].as_str().unwrap_or("").to_string(),
                    (p["status"].as_str().unwrap_or("").to_string(), p["detail"].as_str().unwrap_or("").to_string()),
                );
            }
        }
    m
}

fn print_models(v: &Value, status: &std::collections::HashMap<String, (String, String)>) -> Vec<String> {
    let (default, default_effort) = alice_split(v["default"].as_str().unwrap_or(""));
    let catalog = v["catalog"].as_array().cloned().unwrap_or_default();
    let mut all = Vec::new();
    for p in v["providers"].as_array().cloned().unwrap_or_default() {
        let kind = p["kind"].as_str().unwrap_or("");
        let id = p["id"].as_str().unwrap_or("");
        // A colored dot: ready, not ready, unknown (server not running).
        let dot = match status.get(id).map(|(s, _)| s.as_str()) {
            Some("ok") => ui::mint("●"),
            Some("ng") => ui::rose("●"),
            _ => ui::dim("○"),
        };
        let auth = match ProviderKind::parse(kind) {
            Some(k) if k.needs_api_key() => if p["has_key"] == true { "key set" } else { "no key" },
            Some(k) if k.is_cli() => "cli login",
            _ => "",
        };
        let detail = status.get(id).map(|(_, d)| d.as_str()).filter(|d| !d.is_empty()).map(|d| format!(" · {d}")).unwrap_or_default();
        println!();
        println!("  {dot} {}  {}", ui::bold(id), ui::dim(&format!("{kind}{}{detail}", if auth.is_empty() { String::new() } else { format!(" · {auth}") })));
        for m in p["models"].as_array().cloned().unwrap_or_default() {
            let full = format!("{id}/{}", m.as_str().unwrap_or(""));
            all.push(full.clone());
            let is_default = full == default;
            let info = catalog.iter().find(|c| c["id"] == full.as_str());
            let name = info.and_then(|i| i["name"].as_str()).filter(|n| *n != m.as_str().unwrap_or("")).unwrap_or("");
            let efforts: Vec<String> = info
                .and_then(|i| i["efforts"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|e| e.as_str())
                .map(|e| {
                    let current = if is_default { default_effort } else { None };
                    let dflt = info.and_then(|i| i["default_effort"].as_str());
                    if Some(e) == current.or(dflt) { ui::pink(&format!("[{e}]")) } else { ui::dim(e) }
                })
                .collect();
            let mark = if is_default { ui::pink("★") } else { " ".into() };
            let label = if is_default { ui::bold(&full) } else { full.clone() };
            let name = if name.is_empty() { String::new() } else { format!("  {}", ui::dim(name)) };
            let efforts = if efforts.is_empty() { String::new() } else { format!("  {}", efforts.join(" ")) };
            println!("    {} {mark} {label}{name}{efforts}", ui::lavender(&format!("{:>2}", all.len())));
        }
    }
    if all.is_empty() {
        ui::note("No models yet. Add one with `alice model add` ✦");
    }
    all
}

fn alice_split(id: &str) -> (&str, Option<&str>) {
    crate::config::split_effort(id)
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
    ui::ok(&format!("default model: {}", ui::bold(model)));
    Ok(())
}

async fn model_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] => loop {
            let v = models_json(paths).await?;
            ui::card("✦ Models", &[format!("profile {} · ★ is the default", paths.profile)]);
            let all = print_models(&v, &provider_status(paths).await);
            println!();
            ui::note("<n> [effort]  set the default (e.g. `2 high`)\na  add · r <n>  remove · t <n>  test · f  refresh · Enter  quit");
            let ans = ask("Choice", "");
            let (cmd, rest) = ans.split_once(' ').map(|(x, y)| (x, y.trim())).unwrap_or((ans.as_str(), ""));
            let pick = |s: &str| s.parse::<usize>().ok().and_then(|n| all.get(n.wrapping_sub(1)).cloned());
            match cmd {
                "" | "q" => return Ok(()),
                "a" => model_add(paths).await?,
                "f" => refresh_models(paths).await?,
                "r" => match pick(rest) {
                    Some(m) => remove_provider(paths, m.split('/').next().unwrap_or("")).await?,
                    None => println!("give a number"),
                },
                "t" => match pick(rest) {
                    Some(m) => test_model(paths, &m).await?,
                    None => println!("give a number"),
                },
                n => match pick(n) {
                    Some(m) if rest.is_empty() => set_default(paths, &m).await?,
                    Some(m) => set_default(paths, &format!("{m}#{rest}")).await?,
                    None => println!("unknown choice"),
                },
            }
        },
        ["list" | "ls"] => {
            print_models(&models_json(paths).await?, &provider_status(paths).await);
            Ok(())
        }
        ["add"] => model_add(paths).await,
        ["remove" | "rm", id] => {
            let m = pick_model(paths, id).await?;
            remove_provider(paths, m.split('/').next().unwrap_or(&m)).await
        }
        ["use", id] => set_default(paths, &pick_model(paths, id).await?).await,
        ["use", id, effort] => set_default(paths, &format!("{}#{effort}", alice_split(&pick_model(paths, id).await?).0)).await,
        ["refresh"] => refresh_models(paths).await,
        ["test", id] => test_model(paths, &pick_model(paths, id).await?).await,
        ["login", provider] => login(paths, provider, &[]).await,
        ["login", provider, extra @ ..] => login(paths, provider, extra).await,
        ["logout", provider] => logout(paths, provider).await,
        _ => Err("usage: alice model [list|refresh|add|remove <id>|use <id> [effort]|login <provider>|logout <provider>|test <id>]".into()),
    }
}

/// Ask every provider for its current models (needs the running server).
async fn refresh_models(paths: &Paths) -> CliResult<()> {
    let Backend::Remote(c) = backend(paths).await else {
        return Err("start Alice first (`alice start`); models are listed through the server".into());
    };
    let v = c.post("/models/refresh", &json!({})).await?;
    for r in v["results"].as_array().into_iter().flatten() {
        let id = r["id"].as_str().unwrap_or("");
        match (r["ok"] == true, r["models"].as_u64()) {
            (true, Some(n)) => ui::ok(&format!("{}  {}", ui::bold(id), ui::dim(&format!("{n} models")))),
            (true, None) => ui::warn(&format!("{}  {}", ui::bold(id), ui::dim("lists no models; add them with `alice model add`"))),
            _ => ui::fail(&format!("{}  {}", ui::bold(id), r["error"].as_str().unwrap_or("failed"))),
        }
    }
    Ok(())
}

/// Accept a full model ID, a number from `alice model list`, or a unique
/// model / provider name.
async fn pick_model(paths: &Paths, arg: &str) -> CliResult<String> {
    if let (base, Some(e)) = alice_split(arg) {
        return Ok(format!("{}#{e}", Box::pin(pick_model(paths, base)).await?));
    }
    let v = models_json(paths).await?;
    let all: Vec<String> = v["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| {
            let id = p["id"].as_str().unwrap_or("").to_string();
            p["models"].as_array().into_iter().flatten().filter_map(move |m| m.as_str().map(|m| format!("{id}/{m}"))).collect::<Vec<_>>()
        })
        .collect();
    if let Ok(n) = arg.parse::<usize>() {
        return all.get(n.wrapping_sub(1)).cloned().ok_or_else(|| format!("no model #{n} (see `alice model list`)"));
    }
    if all.iter().any(|m| m == arg) {
        return Ok(arg.to_string());
    }
    let hits: Vec<&String> = all.iter().filter(|m| m.split('/').nth(1) == Some(arg) || m.split('/').next() == Some(arg)).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("unknown model: {arg} (see `alice model list`)")),
        _ => Err(format!("'{arg}' is ambiguous: {}", hits.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))),
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
    ui::ok(&format!("removed {} and its API key", ui::bold(id)));
    Ok(())
}

async fn test_model(paths: &Paths, model: &str) -> CliResult<()> {
    let c = remote(paths).await?;
    ui::note(&format!("testing {model} …"));
    let v = c.post("/models/test", &json!({ "model": model })).await?;
    if v["ok"] == true {
        ui::ok(&format!(
            "{}  {}",
            v["reply"].as_str().unwrap_or("").trim(),
            ui::dim(&format!(
                "first token {} ms · total {} ms",
                v["ttft_ms"].as_u64().map(|n| n.to_string()).unwrap_or("-".into()),
                v["total_ms"]
            ))
        ));
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

/// `codex login` into Alice's own CODEX_HOME (separate from ~/.codex, so
/// your usual Codex setup and login are untouched).
fn codex_login(paths: &Paths, extra: &[&str]) -> CliResult<()> {
    let home = crate::providers::cli::prepare_codex_home()?;
    ui::heading("Sign in to ChatGPT");
    ui::note(&format!("Through the official Codex CLI. This login is Alice's own ({});\nyour ~/.codex is not used.", home.display()));
    if extra.is_empty() && std::env::var_os("SSH_CONNECTION").is_some() {
        ui::note("On a server without a browser: `alice model login codex-cli --device-auth`.");
    }
    let exe = Config::load(paths)?.providers.iter().find(|p| p.kind == ProviderKind::CodexCli).map(|p| p.base_url().to_string()).unwrap_or_else(|| "codex".into());
    let st = std::process::Command::new(&exe).env("CODEX_HOME", &home).arg("login").args(extra).status().map_err(|e| format!("{exe}: {e}"))?;
    if st.success() { Ok(()) } else { Err("codex login failed".into()) }
}

/// Antigravity keeps its own sign-in (OS keyring); `agy` asks for it on first
/// start. On a server it prints a URL to open elsewhere and asks for the code.
fn agy_login(paths: &Paths, provider: &str) -> CliResult<()> {
    let cfg = Config::load(paths).map_err(|e| e.to_string())?;
    let exe = cfg.providers.iter().find(|p| p.id == provider).map(|p| p.base_url().to_string()).unwrap_or_else(|| "agy".into());
    ui::heading("Sign in to Antigravity");
    ui::note(&format!("Starting `{exe}`. Sign in with your Google account.\nOn a server: open the URL it prints on another device, then paste the code back.\nWhen its prompt appears you are signed in: type /exit (or Ctrl+C) to come back."));
    let st = std::process::Command::new(&exe).status().map_err(|e| format!("{exe}: {e} (install: curl -fsSL https://antigravity.google/cli/install.sh | bash)"))?;
    let check = std::process::Command::new(&exe).arg("models").output();
    match check {
        Ok(o) if o.status.success() => {
            ui::ok("signed in ✦ next: `alice model refresh` to load the models");
            Ok(())
        }
        _ if !st.success() => Err("agy did not finish signing in".into()),
        _ => Err("agy is not signed in yet; run it again and finish the sign-in".into()),
    }
}

async fn login(paths: &Paths, provider: &str, extra: &[&str]) -> CliResult<()> {
    match provider_kind(paths, provider)? {
        ProviderKind::CodexCli => codex_login(paths, extra),
        ProviderKind::GeminiCli => Err("gemini-cli is retired (Google ended its subscription login). Add antigravity-cli instead.".into()),
        ProviderKind::AntigravityCli => agy_login(paths, provider),
        ProviderKind::Ollama => {
            ui::ok("Ollama needs no login");
            Ok(())
        }
        _ => {
            let key = ask_secret("API key (input hidden)");
            if key.is_empty() {
                return Err("no key given".into());
            }
            set_secret(paths, &format!("provider.{provider}.api_key"), &key).await?;
            ui::ok(&format!("saved the API key for {}", ui::bold(provider)));
            Ok(())
        }
    }
}

async fn logout(paths: &Paths, provider: &str) -> CliResult<()> {
    match provider_kind(paths, provider)? {
        ProviderKind::CodexCli => {
            let home = crate::providers::cli::prepare_codex_home()?;
            let _ = std::process::Command::new("codex").env("CODEX_HOME", home).arg("logout").status();
            Ok(())
        }
        ProviderKind::GeminiCli => Ok(()),
        ProviderKind::AntigravityCli => {
            println!("Sign out inside the Antigravity CLI: run `agy`, then /logout.");
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

pub use crate::util::urlencode;

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
                    let cat = crate::skills::load_catalog(&paths.skills());
                    if !cat.skills.iter().any(|s| s.id() == name || s.manifest.aliases.contains(&name) || s.tool(&name).is_some()) {
                        return Err(format!("unknown tool or skill: {name}"));
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
            let cat = crate::skills::load_catalog(&paths.skills());
            for s in &cat.skills {
                if !crate::skills::enabled(&cfg, s) {
                    continue;
                }
                println!("{}", s.manifest.name);
                for t in &s.manifest.tools {
                    let p = crate::skills::permission(&cfg, s, t);
                    println!("  {:<20} {:<9} {}", t.name, p.as_str(), dim(&t.description));
                }
            }
            println!("{}", dim(&format!("working folder: {}", crate::tools::fs::workspace_dir(&cfg, paths).display())));
            if !cfg.tools.fs.roots.is_empty() {
                println!("{}", dim(&format!("also allowed: {}", cfg.tools.fs.roots.join(", "))));
            }
            println!("{}", dim("change: alice config set tools.fs.workspace ~/path"));
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

#[cfg(test)]
mod parse_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn login_options_reach_the_provider() {
        let c = Cli::try_parse_from(["alice", "model", "login", "codex-cli", "--device-auth"]).unwrap();
        match c.cmd {
            Some(Cmd::Model { args }) => assert_eq!(args, ["login", "codex-cli", "--device-auth"]),
            _ => panic!("not a model command"),
        }
    }
}
