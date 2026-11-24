//! Alice Agent (A2): a lightweight, local-first personal AI agent.

pub mod util;

pub mod agent;
pub mod app;
pub mod channels;
pub mod cli;
pub mod config;
pub mod memory;
pub mod providers;
pub mod server;
pub mod storage;
pub mod tools;
pub mod usage;

use config::{Config, Paths, Secrets};

/// Load everything and build the shared application state (no network).
pub fn build_app(paths: Paths) -> Result<app::AppRef, String> {
    paths.ensure().map_err(|e| format!("{}: {e}", paths.home.display()))?;
    let mut config = Config::load(&paths)?;
    config::ensure_soul(&paths);
    // Heal a default model that no longer exists (e.g. removed or mistyped).
    if !config.agent.default_model.is_empty() && config.resolve_model(&config.agent.default_model).is_none() {
        let fallback = config.all_models().into_iter().next().unwrap_or_default();
        warn!("default model '{}' is not configured; using '{}'", config.agent.default_model, fallback);
        config.agent.default_model = fallback;
        config.save(&paths)?;
    }
    let mut secrets = Secrets::load(&paths)?;
    if secrets.get("server.token").is_none() {
        secrets.set("server.token", &util::new_token());
        secrets.save(&paths)?;
        info!("generated a new API token (see `alice token`)");
    }
    let db = storage::Db::open(&paths.db()).map_err(|e| format!("database: {e}"))?;
    let n = storage::recover_streaming(&db.lock()).map_err(|e| e.to_string())?;
    if n > 0 {
        info!("marked {n} interrupted generation(s) as cancelled");
    }
    Ok(app::App::new(paths, config, secrets, db))
}

pub async fn serve(paths: Paths) -> Result<(), String> {
    let app = build_app(paths)?;
    channels::start(&app);
    server::serve(app).await.map_err(|e| e.to_string())
}
