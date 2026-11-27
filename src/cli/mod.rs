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
use clap::{Parser, Subcommand};
use client::{CliResult, Client};
use serde_json::{Value, json};

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
            print_usage(&c.get("/usage").await?);
            Ok(())
        }
        Some(Cmd::Serve) => unreachable!(),
        Some(Cmd::Mcp) => crate::mcp::serve_stdio().await,
    }
}

// ---------------------------------------------------------------- prompts

pub fn ask(prompt: &str, default: &str) -> String {
    ui::text(prompt, default)
}

pub fn ask_secret(prompt: &str) -> String {
    ui::password(prompt)
}

pub fn confirm(prompt: &str, default: bool) -> bool {
    ui::confirm(prompt, default)
}

/// Pick one option (label, hint) with the arrow keys.
fn choose_with(prompt: &str, options: &[(&str, &str)], default: usize) -> usize {
    ui::select(prompt, options, default)
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

// ---------------------------------------------------------------- usage

/// Subscription limits as soft bars, then tokens per model.
fn print_usage(v: &Value) {
    ui::intro("Alice · usage");
    let subs = v["subscriptions"].as_array().cloned().unwrap_or_default();
    if subs.is_empty() {
        ui::note("no subscription providers (codex-cli) yet");
    }
    for sub in &subs {
        let plan = sub.pointer("/limits/plan").and_then(|p| p.as_str()).map(|p| format!(" {}", ui::dim(p))).unwrap_or_default();
        ui::heading(&format!("{}{plan}", sub["id"].as_str().unwrap_or("")));
        if sub["ok"] != true {
            ui::warn(sub["error"].as_str().unwrap_or("unavailable"));
            continue;
        }
        for (name, used, reset) in crate::usage::windows(&sub["limits"]) {
            let left = 100.0 - used;
            let filled = ((left / 100.0) * 20.0).round() as usize;
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(20 - filled.min(20)));
            let bar = if left > 50.0 { ui::mint(&bar) } else if left > 20.0 { ui::lemon(&bar) } else { ui::rose(&bar) };
            ui::line(&format!("{name:<7} {bar}  {} {}", ui::bold(&format!("{left:.0}% left")), ui::dim(&format!("· resets {reset}"))));
        }
    }
    for (label, key) in [("Last 24 hours", "day"), ("Last 30 days", "month")] {
        ui::heading(label);
        let rows = v["tokens"][key].as_array().cloned().unwrap_or_default();
        if rows.is_empty() {
            ui::note("—");
        }
        let w = rows.iter().map(|r| r["model"].as_str().unwrap_or("").len()).max().unwrap_or(0);
        for r in rows {
            let m = r["model"].as_str().unwrap_or("");
            ui::line(&format!(
                "{m}{}  {}",
                " ".repeat(w - m.len()),
                ui::dim(&format!(
                    "in {} · out {} · {} replies",
                    crate::usage::compact(r["input_tokens"].as_i64().unwrap_or(0)),
                    crate::usage::compact(r["output_tokens"].as_i64().unwrap_or(0)),
                    r["responses"]
                ))
            ));
        }
    }
    ui::outro(&ui::dim("tokens Alice used, by model"));
}

// ---------------------------------------------------------------- setup

async fn setup(paths: &Paths) -> CliResult<()> {
    const STEPS: usize = 5;
    ui::intro("Alice · setup");
    ui::note(&format!("profile {} · {}", paths.profile, ui::path(&paths.home)));
    ui::note("Enter keeps the value shown in grey ♡");
    paths.ensure().map_err(|e| e.to_string())?;
    let mut cfg = Config::load(paths)?;

    ui::step(1, STEPS, "Server");
    ui::note("127.0.0.1 is always on. Add this machine's LAN or Tailscale IP to reach Alice from other");
    ui::note("devices (comma-separated; 0.0.0.0 inside a container). Plain HTTP: trusted networks only.");
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
    let langs = [("日本語 (ja)", ""), ("English (en)", "")];
    let codes = ["ja", "en"];
    let cur = codes.iter().position(|c| *c == cfg.agent.language).unwrap_or(0);
    cfg.agent.language = codes[choose_with("Reply language", &langs, cur)].to_string();
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
    ui::note("`alice token` shows it: the Web UI and the iPhone app sign in with it.");

    ui::step(3, STEPS, "Personality");
    crate::config::ensure_soul(paths);
    ui::ok(&ui::path(&paths.soul()));
    ui::note("Alice's voice and manners. Edit any time with `alice soul`.");

    ui::step(4, STEPS, "Model");
    if cfg.providers.is_empty() || confirm("Add a model provider now?", cfg.providers.is_empty()) {
        model_add(paths).await?;
    } else {
        ui::ok(&format!("default model {}", ui::bold(&cfg.agent.default_model)));
    }

    ui::step(5, STEPS, "Start automatically");
    if cfg!(any(target_os = "macos", target_os = "linux")) && confirm("Start Alice at login (launchd / systemd)?", true) {
        match service::register(paths) {
            Ok(m) => ui::ok(&m),
            Err(e) => ui::warn(&format!("could not register the service: {e}")),
        }
    }
    ui::heading("All set! ₍ᐢ. .ᐢ₎♡");
    ui::kv(&[
        ("alice start", ui::dim("start the server")),
        ("alice chat", ui::dim("talk with Alice")),
        ("alice token", ui::dim("sign in from Web / iPhone")),
    ]);
    ui::outro(&ui::pink("see you soon ✦"));
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
    ui::intro(&format!("Alice · {}", paths.profile));
    if !c.is_up().await {
        ui::kv(&[
            ("state", ui::tag("stopped", ui::Tone::Off)),
            ("address", c.base.clone()),
            ("service", mgr.to_string()),
        ]);
        ui::outro(&ui::dim("start it with `alice start`"));
        return Ok(());
    }
    let s = c.get("/status").await?;
    let mb = s["rss_kb"].as_f64().unwrap_or(0.0) / 1024.0;
    ui::kv(&[
        ("state", format!("{} {}", ui::tag("running", ui::Tone::Good), ui::dim(&format!("v{}", s["version"].as_str().unwrap_or(""))))),
        ("address", c.base.clone()),
        ("model", ui::bold(s["default_model"].as_str().filter(|m| !m.is_empty()).unwrap_or("(none)"))),
        ("process", ui::dim(&format!("pid {} · up {} · {mb:.1} MB · cpu {}%", s["pid"], human_secs(s["uptime_secs"].as_u64().unwrap_or(0)), s["cpu_percent"]))),
        ("data", ui::dim(&format!(
            "{} threads · {} messages · {} memories · {:.1} KB",
            s["counts"]["threads"], s["counts"]["messages"], s["counts"]["memories"], s["db_bytes"].as_f64().unwrap_or(0.0) / 1024.0
        ))),
        ("service", ui::dim(&format!("{mgr} · {}", s["home"].as_str().unwrap_or("")))),
    ]);
    ui::outro(&ui::pink("all good ♡"));
    Ok(())
}

/// "3d 4h" / "2h 13m" / "45s".
fn human_secs(s: u64) -> String {
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {}s", s / 60, s % 60),
        3600..86_400 => format!("{}h {}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d {}h", s / 86_400, s % 86_400 / 3600),
    }
}

async fn token_cmd(paths: &Paths, rotate: bool, set: bool) -> CliResult<()> {
    let mut s = Secrets::load(paths)?;
    ui::intro("Alice · sign-in");
    if set {
        let t = ask_secret("New API token");
        crate::util::check_token(&t)?;
        if ask_secret("Once more") != t {
            return Err("the two entries differ; nothing changed".into());
        }
        s.set("server.token", &t);
        s.save(paths)?;
        ui::ok("token saved · in use right away (sign in again on the Web and iPhone)");
    } else if rotate || s.get("server.token").is_none() {
        s.set("server.token", &crate::util::new_token());
        s.save(paths)?;
        ui::ok("new token made · in use right away (sign in again on the Web and iPhone)");
    }
    let cfg = Config::load(paths)?;
    let mut rows: Vec<(&str, String)> = vec![("profile", paths.profile.clone())];
    if !cfg.server.public_url.is_empty() {
        rows.push(("server", ui::bold(&cfg.server.public_url)));
    } else {
        // Every address other devices can use, then this machine's own.
        for a in crate::server::listen_addrs(&cfg.server.bind).into_iter().filter(|a| !matches!(a.as_str(), "127.0.0.1" | "localhost" | "::1")) {
            let u = match a.as_str() {
                "0.0.0.0" | "::" => format!("http://<this machine's IP>:{}", cfg.server.port),
                h if h.contains(':') => format!("http://[{h}]:{}", cfg.server.port),
                h => format!("http://{h}:{}", cfg.server.port),
            };
            rows.push(("server", ui::bold(&u)));
        }
        rows.push(("server", format!("{} {}", crate::cli::client::local_url(&cfg), ui::dim("(this machine only)"))));
    }
    if !set {
        rows.push(("token", ui::pink(s.get("server.token").unwrap_or(""))));
    }
    ui::kv(&rows);
    ui::outro(&ui::dim("paste these into the Web UI or the iPhone app ♡"));
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
            ui::ok(&format!("saved {} · Alice uses it from the next reply ♡", ui::path(&file)));
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
            ui::intro("Alice · personality");
            if !confirm("Replace SOUL.md with the default template?", false) {
                ui::outro(&ui::dim("left as it was"));
                return Ok(());
            }
            crate::config::write_atomic(&file, crate::config::SOUL_TEMPLATE.as_bytes(), 0o600)?;
            ui::ok(&format!("reset {}", ui::path(&file)));
            ui::outro(&ui::pink("✦"));
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
        "ready" => ui::tag("ready", ui::Tone::Good),
        "disabled" => ui::tag("off", ui::Tone::Off),
        "needs_setup" => ui::tag("needs setup", ui::Tone::Warn),
        st => ui::tag(st, ui::Tone::Bad),
    }
}

async fn skill_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let c = remote(paths).await?;
    match a.as_slice() {
        [] | ["list" | "ls"] => {
            let v = c.get("/skills").await?;
            ui::intro("Alice · skills");
            let list: Vec<&Value> = v["skills"].as_array().into_iter().flatten().collect();
            let w = list.iter().map(|s| s["id"].as_str().unwrap_or("").len()).max().unwrap_or(0);
            for s in &list {
                let id = s["id"].as_str().unwrap_or("");
                let tools = s["tools"].as_array().map(|t| t.len()).unwrap_or(0);
                let src = if s["source"] == "builtin" { "built-in" } else { "yours" };
                ui::line(&format!("{} {}{}  {}", ui::pink("✦"), ui::bold(id), " ".repeat(w - id.len()), status_mark(s)));
                ui::note(&format!("    {} · {tools} tools · {src}", ui::clip(s["description"].as_str().unwrap_or(""), 64)));
            }
            for i in v["invalid"].as_array().into_iter().flatten() {
                ui::fail(&format!("{}  {}", ui::bold(i["folder"].as_str().unwrap_or("")), i["error"].as_str().unwrap_or("")));
            }
            ui::outro(&ui::dim(&format!("`alice skill info <id>` for details · folder {}", v["dir"].as_str().unwrap_or(""))));
            Ok(())
        }
        ["info" | "show", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            ui::intro(&format!("{} · skill", s["name"].as_str().unwrap_or(id)));
            ui::line(&format!("{}  {}", status_mark(s), ui::dim(&format!("v{} · {}", s["version"].as_str().unwrap_or(""), s["source"].as_str().unwrap_or("")))));
            ui::note(s["description"].as_str().unwrap_or(""));
            let p = &s["permissions"];
            let net: Vec<&str> = p["network"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
            ui::heading("What it may do");
            ui::kv(&[
                ("network", if net.is_empty() { ui::dim("none") } else { net.join(", ") }),
                ("files", if p["files"] == true { "yes".into() } else { ui::dim("no") }),
            ]);
            if let Some(d) = p["description"].as_str().filter(|d| !d.is_empty()) {
                ui::note(d);
            }
            if let Some(au) = s["auth"].as_object() {
                ui::heading("Sign-in");
                ui::kv(&[(
                    au["label"].as_str().unwrap_or("account"),
                    if au["connected"] == true { ui::tag("connected", ui::Tone::Good) } else { format!("{} {}", ui::tag("not connected", ui::Tone::Warn), ui::dim(&format!("alice skill login {id}"))) },
                )]);
            }
            let settings: Vec<&Value> = s["settings"].as_array().into_iter().flatten().filter(|x| x["advanced"] != true).collect();
            if !settings.is_empty() {
                ui::heading("Settings");
                let rows: Vec<(&str, String)> = settings
                    .iter()
                    .map(|x| {
                        let val = if x["secret"] == true {
                            if x["set"] == true { "••••".to_string() } else { ui::dim("(not set)") }
                        } else {
                            x["value"].as_str().filter(|v| !v.is_empty()).map(str::to_string).unwrap_or_else(|| ui::dim("(not set)"))
                        };
                        (x["key"].as_str().unwrap_or(""), val)
                    })
                    .collect();
                ui::kv(&rows);
            }
            if let Some(g) = s["groups"].as_array().filter(|g| !g.is_empty()) {
                ui::heading("Groups");
                let on: Vec<String> = g
                    .iter()
                    .map(|x| if x["enabled"] == true { format!("{} {}", ui::mint("◼"), x["id"].as_str().unwrap_or("")) } else { format!("{} {}", ui::dim("◻"), ui::dim(x["id"].as_str().unwrap_or(""))) })
                    .collect();
                ui::line(&on.join("   "));
            }
            ui::heading("Tools");
            let tools: Vec<&Value> = s["tools"].as_array().into_iter().flatten().collect();
            let w = tools.iter().map(|t| t["name"].as_str().unwrap_or("").len()).max().unwrap_or(0);
            for t in tools {
                let name = t["name"].as_str().unwrap_or("");
                let perm = match t["permission"].as_str().unwrap_or("") {
                    "allow" => ui::tag("allow", ui::Tone::Good),
                    "ask" => ui::tag("ask", ui::Tone::Info),
                    p => ui::tag(p, ui::Tone::Off),
                };
                let avail = if t["available"] == true { String::new() } else { ui::dim(&format!(" ({})", t["reason"].as_str().unwrap_or(""))) };
                ui::line(&format!("{name}{}  {}  {}{avail}", " ".repeat(w - name.len()), ui::pad(&perm, 7), ui::dim(&ui::clip(t["description"].as_str().unwrap_or(""), 56))));
            }
            ui::outro(&ui::dim("change a tool with `alice tool enable <tool> ask|allow`"));
            Ok(())
        }
        ["enable", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            if s["source"] != "builtin" {
                let net: Vec<&str> = s["permissions"]["network"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
                ui::intro(&format!("Enable {}", s["name"].as_str().unwrap_or(id)));
                ui::heading("It asks for");
                ui::kv(&[
                    ("network", if net.is_empty() { ui::dim("none") } else { net.join(", ") }),
                    ("files", if s["permissions"]["files"] == true { "yes".into() } else { ui::dim("no") }),
                ]);
                if s["tools"].as_array().into_iter().flatten().any(|t| t["handler"] == "command") {
                    ui::warn("runs local programs from its folder");
                }
                if !confirm("Enable it?", false) {
                    ui::outro(&ui::dim("left off"));
                    return Ok(());
                }
                c.patch(&format!("/skills/{id}"), &json!({ "enabled": true })).await?;
                ui::ok(&format!("{} enabled", ui::bold(id)));
                ui::outro(&ui::pink("✦"));
                return Ok(());
            }
            c.patch(&format!("/skills/{id}"), &json!({ "enabled": true })).await?;
            ui::ok(&format!("{} enabled", ui::bold(id)));
            Ok(())
        }
        ["disable", id] => {
            c.patch(&format!("/skills/{id}"), &json!({ "enabled": false })).await?;
            ui::ok(&format!("{} turned off", ui::bold(id)));
            Ok(())
        }
        ["set", id, key, value @ ..] => {
            c.patch(&format!("/skills/{id}"), &json!({ "settings": { *key: value.join(" ") } })).await?;
            ui::ok(&format!("{} saved", ui::bold(&format!("{id}.{key}"))));
            Ok(())
        }
        ["groups", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            let g: Vec<&Value> = s["groups"].as_array().into_iter().flatten().collect();
            if g.is_empty() {
                return Err(format!("{id} has no groups"));
            }
            ui::intro(&format!("{} · groups", s["name"].as_str().unwrap_or(id)));
            let items: Vec<(&str, &str)> = g.iter().map(|x| (x["id"].as_str().unwrap_or(""), x["label"].as_str().unwrap_or(""))).collect();
            let on: Vec<bool> = g.iter().map(|x| x["enabled"] == true).collect();
            let picked = ui::multiselect("Turn on", &items, &on);
            let groups: Vec<&str> = items.iter().zip(&picked).filter(|(_, p)| **p).map(|((i, _), _)| *i).collect();
            c.patch(&format!("/skills/{id}"), &json!({ "groups": groups })).await?;
            ui::ok("saved");
            ui::outro(&ui::dim(&format!("sign in again so the new groups are granted: alice skill login {id}")));
            Ok(())
        }
        ["groups", id, list] => {
            let groups: Vec<&str> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
            c.patch(&format!("/skills/{id}"), &json!({ "groups": groups })).await?;
            ui::ok(&format!("{}: {}", ui::bold(id), groups.join(", ")));
            Ok(())
        }
        ["setup", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            ui::intro(&format!("Set up {}", s["name"].as_str().unwrap_or(id)));
            if *id == "google-workspace" {
                ui::note("Create an OAuth client of type \"Desktop app\" in Google Cloud Console");
                ui::note("(APIs & Services → Credentials) and enable the APIs you want to use.");
                ui::note("While the consent screen is in \"Testing\", Google ends the sign-in after 7 days.");
            }
            let mut settings = serde_json::Map::new();
            for x in s["settings"].as_array().into_iter().flatten().filter(|x| x["advanced"] != true) {
                let key = x["key"].as_str().unwrap_or("");
                let label = x["label"].as_str().filter(|l| !l.is_empty()).unwrap_or(key);
                let val = if x["secret"] == true {
                    ask_secret(&format!("{label} (Enter keeps the current one)"))
                } else {
                    ask(label, x["value"].as_str().unwrap_or(""))
                };
                if !val.is_empty() {
                    settings.insert(key.to_string(), json!(val));
                }
            }
            let mut body = json!({ "settings": settings, "enabled": true });
            if let Some(g) = s["groups"].as_array().filter(|g| !g.is_empty()) {
                let items: Vec<(&str, &str)> = g.iter().map(|x| (x["id"].as_str().unwrap_or(""), x["label"].as_str().unwrap_or(""))).collect();
                let on: Vec<bool> = g.iter().map(|x| x["enabled"] == true).collect();
                let picked = ui::multiselect("Turn on", &items, &on);
                body["groups"] = json!(items.iter().zip(&picked).filter(|(_, p)| **p).map(|((i, _), _)| *i).collect::<Vec<_>>());
            }
            c.patch(&format!("/skills/{id}"), &body).await?;
            ui::ok("saved");
            if s["auth"].is_object() && confirm("Sign in now?", true) {
                return Box::pin(skill_cmd(paths, vec!["login".into(), id.to_string()])).await;
            }
            ui::outro(&ui::pink("✦"));
            Ok(())
        }
        ["login", id] => {
            let v = c.get("/skills").await?;
            let s = find_skill(&v, id)?;
            match s["auth"]["type"].as_str() {
                Some("token") => {
                    let t = ask_secret(s["auth"]["label"].as_str().unwrap_or("Token"));
                    c.post(&format!("/skills/{id}/login"), &json!({ "token": t })).await?;
                    ui::ok(&format!("{} connected", ui::bold(id)));
                    Ok(())
                }
                Some("oauth2") => {
                    let r = c.post(&format!("/skills/{id}/login"), &json!({})).await?;
                    let url = r["auth_url"].as_str().unwrap_or("");
                    ui::heading(&format!("Sign in to {}", s["name"].as_str().unwrap_or(id)));
                    ui::note("Open this URL in a browser and approve access:");
                    ui::line(&ui::sky(url));
                    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
                    let _ = std::process::Command::new(opener).arg(url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
                    ui::note("If the browser is on another machine, its last page will not load:");
                    ui::note("copy that page's full URL from the address bar and paste it here.");
                    ui::gap();
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
                            ui::ok(&format!("{} connected ✦", ui::bold(id)));
                            return Ok(());
                        }
                    }
                    Err("timed out waiting for the sign-in".into())
                }
                _ => Err(format!("{id} has no sign-in")),
            }
        }
        ["logout", id] => {
            c.post(&format!("/skills/{id}/logout"), &json!({})).await?;
            ui::ok(&format!("{} disconnected (credentials removed)", ui::bold(id)));
            Ok(())
        }
        ["rescan"] => {
            c.post("/skills/rescan", &json!({})).await?;
            Box::pin(skill_cmd(paths, vec![])).await
        }
        ["log"] | ["log", _] => {
            let n = a.get(1).and_then(|n| n.parse::<usize>().ok()).unwrap_or(30);
            let v = c.get(&format!("/tool-runs?limit={n}")).await?;
            ui::intro("Alice · tool runs");
            for r in v["runs"].as_array().into_iter().flatten().rev() {
                let st = r["status"].as_str().unwrap_or("");
                let st = match st {
                    "ok" => ui::tag(st, ui::Tone::Good),
                    "error" | "denied" | "unavailable" => ui::tag(st, ui::Tone::Bad),
                    _ => ui::tag(st, ui::Tone::Warn),
                };
                ui::line(&format!(
                    "{}  {} {}  {}  {}",
                    ui::dim(&crate::util::fmt_utc(r["ts"].as_i64().unwrap_or(0))[5..16]),
                    ui::bold(r["tool"].as_str().unwrap_or("")),
                    st,
                    ui::dim(&format!("{} · {} · {}ms", r["skill"].as_str().unwrap_or(""), r["source"].as_str().unwrap_or(""), r["duration_ms"])),
                    ui::rose(r["error"].as_str().unwrap_or(""))
                ));
            }
            ui::outro(&ui::dim("times are UTC"));
            Ok(())
        }
        _ => Err("usage: alice skill [list | info <id> | enable <id> | disable <id> | setup <id> | set <id> <key> <value> | groups <id> a,b | login <id> | logout <id> | log [n] | rescan]".into()),
    }
}

fn service_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    match args.first().map(String::as_str) {
        Some("install") => {
            ui::ok(&service::register(paths)?);
            Ok(())
        }
        Some("uninstall") => {
            service::unregister(paths)?;
            ui::ok(&format!("start-at-login removed for profile {}", ui::bold(&paths.profile)));
            Ok(())
        }
        None | Some("status") => {
            let (state, file) = match service::manager(paths) {
                service::Manager::Launchd => (ui::tag("launchd", ui::Tone::Good), ui::path(&service::launchd_plist(paths))),
                service::Manager::Systemd => (ui::tag("systemd", ui::Tone::Good), ui::path(&service::systemd_unit(paths))),
                service::Manager::None => (ui::tag("not registered", ui::Tone::Off), "`alice service install` to start at login".into()),
            };
            ui::intro("Alice · start at login");
            ui::kv(&[("profile", paths.profile.clone()), ("service", state), ("file", ui::dim(&file))]);
            ui::outro("");
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
            ui::ok(&format!("deleted {} and the memories learned from it", ui::bold(&args[1])));
            Ok(())
        }
        (Some("rename"), n) if n >= 2 => {
            let title = if n > 2 { args[2..].join(" ") } else { ask("New title", "") };
            c.patch(&format!("/threads/{}", args[1]), &json!({ "title": title })).await?;
            ui::ok(&format!("renamed to {}", ui::bold(&title)));
            Ok(())
        }
        (Some("clear"), 1) => {
            ui::intro("Delete every chat");
            ui::warn("This deletes ALL threads and every memory Alice learned from them.");
            ui::note("Settings, models, API keys and connections are kept.");
            if ask("Type delete-all to confirm", "") != "delete-all" {
                ui::outro(&ui::dim("nothing deleted"));
                return Ok(());
            }
            c.delete("/threads", Some(&json!({ "confirm": "delete-all" }))).await?;
            ui::ok("every chat and the memories learned from them are gone");
            ui::outro(&ui::dim("a fresh start ✦"));
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
        let state = match status.get(id).map(|(s, _)| s.as_str()) {
            Some("ok") => ui::tag("ready", ui::Tone::Good),
            Some("ng") => ui::tag("not ready", ui::Tone::Bad),
            _ => ui::tag("server off", ui::Tone::Off),
        };
        let auth = match ProviderKind::parse(kind) {
            Some(k) if k.needs_api_key() => if p["has_key"] == true { "key set" } else { "no key" },
            Some(k) if k.is_cli() => "cli login",
            _ => "",
        };
        if !all.is_empty() {
            ui::gap();
        }
        ui::line(&format!("{} {}  {}", ui::bold(id), state, ui::dim(&format!("{kind}{}", if auth.is_empty() { String::new() } else { format!(" · {auth}") }))));
        if let Some((_, d)) = status.get(id).filter(|(_, d)| !d.is_empty()) {
            ui::note(&format!("  {d}"));
        }
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
                    if Some(e) == current.or(dflt) { ui::pink(e) } else { ui::dim(e) }
                })
                .collect();
            let mark = if is_default { ui::pink("★") } else { ui::dim("·") };
            let label = if is_default { ui::bold(m.as_str().unwrap_or("")) } else { m.as_str().unwrap_or("").to_string() };
            let name = if name.is_empty() { String::new() } else { format!("  {}", ui::dim(name)) };
            let efforts = if efforts.is_empty() { String::new() } else { format!("  {}", efforts.join(ui::dim(" · ").as_str())) };
            ui::line(&format!("  {mark} {label}{name}{efforts}"));
        }
    }
    if all.is_empty() {
        ui::gap();
        ui::note("No models yet. Add one with `alice model add` ✦");
    }
    all
}

/// The models with their display names, for pickers.
fn model_rows(v: &Value) -> Vec<(String, String)> {
    let catalog = v["catalog"].as_array().cloned().unwrap_or_default();
    let default = v["default"].as_str().map(|d| alice_split(d).0.to_string()).unwrap_or_default();
    v["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| {
            let id = p["id"].as_str().unwrap_or("").to_string();
            let catalog = &catalog;
            let default = &default;
            p["models"].as_array().into_iter().flatten().filter_map(move |m| {
                let full = format!("{id}/{}", m.as_str()?);
                let name = catalog.iter().find(|c| c["id"] == full.as_str()).and_then(|i| i["name"].as_str()).unwrap_or("").to_string();
                let hint = if &full == default { format!("{name} ★ default").trim().to_string() } else { name };
                Some((full, hint))
            })
        })
        .collect()
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
        [] => {
            ui::intro("Alice · models");
            loop {
                let v = models_json(paths).await?;
                let status = provider_status(paths).await;
                print_models(&v, &status);
                let rows = model_rows(&v);
                let actions: [(&str, &str); 7] = [
                    ("Set the default model", "the one new chats use"),
                    ("Add a provider", "API key, local or subscription"),
                    ("Test a model", "send a tiny message"),
                    ("Sign in", "subscription CLIs and API keys"),
                    ("Refresh model lists", "ask every provider again"),
                    ("Remove a provider", ""),
                    ("Done", ""),
                ];
                ui::gap();
                let pick_model = |prompt: &str| {
                    let items: Vec<(&str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
                    let d = rows.iter().position(|(_, h)| h.contains("default")).unwrap_or(0);
                    (!items.is_empty()).then(|| rows[choose_with(prompt, &items, d)].0.clone())
                };
                match choose_with("What would you like to do?", &actions, 0) {
                    0 => {
                        let Some(m) = pick_model("Model") else { continue };
                        let efforts: Vec<String> = v["catalog"]
                            .as_array()
                            .and_then(|c| c.iter().find(|x| x["id"] == m.as_str()))
                            .and_then(|i| i["efforts"].as_array())
                            .map(|e| e.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                            .unwrap_or_default();
                        if efforts.is_empty() {
                            set_default(paths, &m).await?;
                        } else {
                            let mut items: Vec<(&str, &str)> = vec![("auto", "the model's own default")];
                            items.extend(efforts.iter().map(|e| (e.as_str(), "")));
                            let e = choose_with("Reasoning effort", &items, 0);
                            set_default(paths, &if e == 0 { m } else { format!("{m}#{}", efforts[e - 1]) }).await?;
                        }
                    }
                    1 => model_add(paths).await?,
                    2 => {
                        if let Some(m) = pick_model("Model to test")
                            && let Err(e) = test_model(paths, &m).await {
                                ui::fail(&e);
                            }
                    }
                    3 => {
                        let ids: Vec<String> = v["providers"].as_array().into_iter().flatten().filter_map(|p| p["id"].as_str().map(str::to_string)).collect();
                        if ids.is_empty() {
                            ui::note("no providers yet");
                            continue;
                        }
                        let items: Vec<(&str, &str)> = ids.iter().map(|i| (i.as_str(), "")).collect();
                        let id = ids[choose_with("Provider", &items, 0)].clone();
                        if let Err(e) = login(paths, &id, &[]).await {
                            ui::fail(&e);
                        }
                    }
                    4 => {
                        if let Err(e) = refresh_models(paths).await {
                            ui::fail(&e);
                        }
                    }
                    5 => {
                        let ids: Vec<String> = v["providers"].as_array().into_iter().flatten().filter_map(|p| p["id"].as_str().map(str::to_string)).collect();
                        if ids.is_empty() {
                            continue;
                        }
                        let items: Vec<(&str, &str)> = ids.iter().map(|i| (i.as_str(), "")).collect();
                        let id = ids[choose_with("Provider to remove", &items, 0)].clone();
                        if confirm(&format!("Remove {id} and its API key?"), false) {
                            remove_provider(paths, &id).await?;
                        }
                    }
                    _ => {
                        ui::outro(&ui::pink("have fun ♡"));
                        return Ok(());
                    }
                }
            }
        }
        ["list" | "ls"] => {
            ui::intro("Alice · models");
            print_models(&models_json(paths).await?, &provider_status(paths).await);
            ui::outro(&ui::dim("★ is the default · `alice model` to change things"));
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
    let sp = ui::spinner("asking every provider for its models");
    let v = c.post("/models/refresh", &json!({})).await;
    sp.clear();
    let v = v?;
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
    let sp = ui::spinner(&format!("saying hi to {model}"));
    let v = c.post("/models/test", &json!({ "model": model })).await;
    sp.clear();
    let v = v?;
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

fn print_memories(v: &Value, title: &str) {
    let list = v["memories"].as_array().cloned().unwrap_or_default();
    ui::intro(title);
    if list.is_empty() {
        ui::note("nothing here yet ☁️");
    }
    for m in &list {
        ui::line(&format!("{} {}", ui::lavender(&format!("{:>4}", m["id"])), m["content"].as_str().unwrap_or("")));
        ui::note(&format!("     {} · {} source(s)", m["key"].as_str().unwrap_or(""), m["source_count"]));
    }
    ui::outro(&ui::dim(&format!("{} memories · `alice memory show <id>` for where they came from", list.len())));
}

async fn memory_cmd(paths: &Paths, args: Vec<String>) -> CliResult<()> {
    let c = Client::from_env(paths)?;
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] | ["list" | "ls"] => {
            print_memories(&c.get("/memories?limit=200").await?, "Alice · memories");
            Ok(())
        }
        ["search", rest @ ..] if !rest.is_empty() => {
            let q = rest.join(" ");
            let url = format!("/memories?q={}", urlencode(&q));
            print_memories(&c.get(&url).await?, &format!("memories · “{q}”"));
            Ok(())
        }
        ["show", id] => {
            let v = c.get(&format!("/memories/{id}")).await?;
            ui::intro(&format!("memory {id}"));
            ui::line(&ui::bold(v["memory"]["content"].as_str().unwrap_or("")));
            ui::note(&format!("key {}", v["memory"]["key"].as_str().unwrap_or("")));
            ui::heading("Where it came from");
            for s in v["sources"].as_array().cloned().unwrap_or_default() {
                ui::line(&format!("{} {}", ui::pink("✦"), s["thread_title"].as_str().filter(|t| !t.is_empty()).unwrap_or("(untitled)")));
                ui::note(&format!("  “{}”", s["content"].as_str().unwrap_or("")));
                ui::note(&format!("  thread {} · message {}", s["thread_id"].as_str().unwrap_or(""), s["message_id"].as_str().unwrap_or("")));
            }
            ui::outro("");
            Ok(())
        }
        ["status"] => {
            let v = c.get("/memories/status").await?;
            let s = &v["status"];
            ui::intro("Alice · memory");
            ui::kv(&[
                ("memories", format!("{}", s["memories"])),
                ("sources", format!("{} from {} threads", s["sources"], s["threads"])),
                ("storage", ui::dim(&format!("db {:.1} KB · attachments {:.1} KB", s["db_bytes"].as_f64().unwrap_or(0.0) / 1024.0, s["attachments_bytes"].as_f64().unwrap_or(0.0) / 1024.0))),
                ("learning", if v["auto_extract"] == true { ui::tag("on", ui::Tone::Good) } else { ui::tag("off", ui::Tone::Off) }),
                ("per reply", format!("up to {} memories", v["inject_limit"])),
            ]);
            ui::outro("");
            Ok(())
        }
        ["clear"] => {
            ui::intro("Forget everything");
            ui::warn("This deletes all memories. Chats stay, and old messages are not learned again.");
            if !confirm("Delete all memories?", false) {
                ui::outro(&ui::dim("kept"));
                return Ok(());
            }
            c.delete("/memories", Some(&json!({ "confirm": "clear-memories" }))).await?;
            ui::ok("all memories deleted");
            ui::outro(&ui::dim("a clean slate ✦"));
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
            ui::intro("Alice · settings");
            let text = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
            for l in text.lines() {
                if l.starts_with('[') {
                    ui::gap();
                    ui::line(&ui::pink(l));
                } else if let Some((k, v)) = l.split_once(" = ") {
                    ui::line(&format!("{} {} {}", ui::lavender(k), ui::dim("="), v));
                } else if !l.trim().is_empty() {
                    ui::line(l);
                }
            }
            ui::outro(&ui::dim(&format!("{} · secrets live in secrets.toml · `alice config set <key> <value>`", ui::path(&paths.config()))));
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
            ui::ok(&format!("{} {} {}", ui::lavender(key), ui::dim("="), ui::bold(&value)));
            if key.starts_with("server.") {
                ui::note("restart the server to apply it (`alice restart`)");
            }
            Ok(())
        }
        ["secret", name] => {
            let value = ask_secret(&format!("{name} (empty removes it)"));
            set_secret(paths, name, &value).await?;
            ui::ok(&format!("{} {}", ui::bold(name), if value.is_empty() { "removed" } else { "saved" }));
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
            ui::intro("Alice · tools");
            for s in &cat.skills {
                if !crate::skills::enabled(&cfg, s) {
                    continue;
                }
                ui::heading(&s.manifest.name);
                let w = s.manifest.tools.iter().map(|t| t.name.len()).max().unwrap_or(0);
                for t in &s.manifest.tools {
                    let p = match crate::skills::permission(&cfg, s, t) {
                        Permission::Allow => ui::tag("allow", ui::Tone::Good),
                        Permission::Ask => ui::tag("ask", ui::Tone::Info),
                        Permission::Disabled => ui::tag("off", ui::Tone::Off),
                    };
                    ui::line(&format!("{}{}  {}  {}", t.name, " ".repeat(w - t.name.len()), ui::pad(&p, 7), ui::dim(&ui::clip(&t.description, 56))));
                }
            }
            ui::gap();
            ui::kv(&[
                ("working folder", ui::path(&crate::tools::fs::workspace_dir(&cfg, paths))),
                ("also allowed", if cfg.tools.fs.roots.is_empty() { ui::dim("—") } else { cfg.tools.fs.roots.join(", ") }),
            ]);
            ui::outro(&ui::dim("`alice tool enable <tool> ask|allow` · `alice tool disable <tool>`"));
            Ok(())
        }
        ["enable", name] => {
            set(name, Some(Permission::Allow)).await?;
            ui::ok(&format!("{} {}", ui::bold(name), ui::tag("allow", ui::Tone::Good)));
            Ok(())
        }
        ["enable", name, mode] => {
            let p = Permission::parse(mode).filter(|p| *p != Permission::Disabled).ok_or("mode must be ask or allow")?;
            set(name, Some(p)).await?;
            ui::ok(&format!("{} {}", ui::bold(name), ui::tag(p.as_str(), ui::Tone::Info)));
            Ok(())
        }
        ["disable", name] => {
            set(name, Some(Permission::Disabled)).await?;
            ui::ok(&format!("{} {}", ui::bold(name), ui::tag("off", ui::Tone::Off)));
            Ok(())
        }
        ["reset", name] => {
            set(name, None).await?;
            ui::ok(&format!("{} back to its default", ui::bold(name)));
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
