//! `alice profile`: independent Alice instances (own config, secrets,
//! models, memory, gateways, port and service) under `~/.alice/profiles/`.

use super::client::{CliResult, Client};
use super::{ask, service};
use crate::config::{Config, DEFAULT_PROFILE, Paths, Secrets, alice_root, current_profile, valid_profile_name};
use std::path::PathBuf;

const RESERVED: &[&str] = &["list", "ls", "use", "delete", "rm", "current", "help"];

pub fn all_profiles() -> Vec<String> {
    let mut names = vec![DEFAULT_PROFILE.to_string()];
    if let Ok(rd) = std::fs::read_dir(alice_root().join("profiles")) {
        let mut more: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().join("config.toml").exists())
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| valid_profile_name(n))
            .collect();
        more.sort();
        names.extend(more);
    }
    names
}

fn exists(name: &str) -> bool {
    name == DEFAULT_PROFILE || Paths::profile(name).config().exists()
}

fn set_current(name: &str) -> CliResult<()> {
    let root = alice_root();
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let f: PathBuf = root.join("current_profile");
    if name == DEFAULT_PROFILE {
        let _ = std::fs::remove_file(f);
        Ok(())
    } else {
        std::fs::write(f, format!("{name}\n")).map_err(|e| e.to_string())
    }
}

/// Next port not used by any existing profile.
fn free_port() -> u16 {
    let used: Vec<u16> = all_profiles()
        .iter()
        .filter_map(|n| Config::load(&Paths::profile(n)).ok())
        .map(|c| c.server.port)
        .collect();
    (7878..u16::MAX).find(|p| !used.contains(p)).unwrap_or(7999)
}

pub fn create(name: &str) -> CliResult<Paths> {
    if !valid_profile_name(name) || RESERVED.contains(&name) {
        return Err(format!("invalid profile name '{name}' (use a-z, 0-9, '-', '_'; max 32)"));
    }
    let paths = Paths::profile(name);
    if paths.config().exists() {
        return Err(format!("profile '{name}' already exists"));
    }
    paths.ensure().map_err(|e| e.to_string())?;
    let mut cfg = Config::default();
    cfg.server.port = free_port();
    // Carry over only harmless personal preferences, nothing else.
    if let Ok(base) = Config::load(&Paths::profile(DEFAULT_PROFILE)) {
        cfg.agent.language = base.agent.language;
        cfg.agent.appearance = base.agent.appearance;
    }
    cfg.save(&paths)?;
    let mut s = Secrets::default();
    s.set("server.token", &crate::util::new_token());
    s.save(&paths)?;
    Ok(paths)
}

pub async fn cmd(current: &Paths, args: Vec<String>) -> CliResult<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] | ["list" | "ls"] => list(current).await,
        ["current"] => {
            println!("{}", current.profile);
            Ok(())
        }
        ["use", name] => {
            if !exists(name) {
                return Err(format!("no such profile: {name} (create it with `alice profile {name}`)"));
            }
            set_current(name)?;
            println!("Now using profile '{name}'.");
            Ok(())
        }
        ["delete" | "rm", name] => delete(name).await,
        [name] => {
            if exists(name) {
                set_current(name)?;
                println!("Now using profile '{name}'.");
                return Ok(());
            }
            let paths = create(name)?;
            set_current(name)?;
            let cfg = Config::load(&paths)?;
            println!("Created profile '{name}' (port {}, data {}).", cfg.server.port, paths.home.display());
            println!("It has its own models, settings, memory and gateways. Now using '{name}'.");
            println!("Next: `alice model add`, then `alice start` (or `alice setup` for the full wizard).");
            println!("Switch back with `alice profile default`; use one-off with `alice -p {name} chat`.");
            Ok(())
        }
        _ => Err("usage: alice profile [list | <name> | use <name> | delete <name> | current]".into()),
    }
}

async fn list(current: &Paths) -> CliResult<()> {
    let selected = current_profile();
    for name in all_profiles() {
        let paths = Paths::profile(&name);
        let cfg = Config::load(&paths).unwrap_or_default();
        let running = match Client::from_env(&paths) {
            Ok(c) => c.is_up().await,
            Err(_) => false,
        };
        let mark = if name == current.profile { "*" } else if name == selected { "+" } else { " " };
        let model = if cfg.agent.default_model.is_empty() { "(no model)".to_string() } else { cfg.agent.default_model.clone() };
        println!(
            "{mark} {name:<16} port {:<5} {:<8} {}",
            cfg.server.port,
            if running { "running" } else { "stopped" },
            super::chat::dim(&model)
        );
    }
    Ok(())
}

async fn delete(name: &str) -> CliResult<()> {
    if name == DEFAULT_PROFILE {
        return Err("the default profile cannot be deleted (use `alice chat clear` to reset its conversations)".into());
    }
    let paths = Paths::profile(name);
    if !paths.config().exists() {
        return Err(format!("no such profile: {name}"));
    }
    println!("This permanently deletes profile '{name}': its conversations, memories, settings, API keys and gateways.");
    if ask(&format!("Type '{name}' to confirm"), "") != name {
        println!("Cancelled.");
        return Ok(());
    }
    if let Ok(c) = Client::from_env(&paths) {
        if c.is_up().await {
            service::stop(&paths).await?;
        }
    }
    service::unregister(&paths)?;
    std::fs::remove_dir_all(&paths.home).map_err(|e| e.to_string())?;
    if current_profile() == name {
        set_current(DEFAULT_PROFILE)?;
        println!("Switched back to the default profile.");
    }
    println!("Deleted profile '{name}'.");
    Ok(())
}
