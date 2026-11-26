//! Alice Agent (A2): a lightweight, local-first personal AI agent.

pub mod util;

pub mod agent;
pub mod app;
pub mod blurb;
pub mod channels;
pub mod cli;
pub mod config;
pub mod mcp;
pub mod memory;
pub mod providers;
pub mod schedule;
pub mod server;
pub mod skills;
pub mod storage;
pub mod tools;
pub mod usage;

use config::{Config, Paths, Secrets};

/// Load everything and build the shared application state (no network).
pub fn build_app(paths: Paths) -> Result<app::AppRef, String> {
    paths.ensure().map_err(|e| format!("{}: {e}", paths.home.display()))?;
    let mut config = Config::load(&paths)?;
    config::ensure_soul(&paths);
    providers::cli::set_codex_home(paths.codex_home());
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
    migrate_google(&paths, &mut config, &mut secrets)?;
    let db = storage::Db::open(&paths.db()).map_err(|e| format!("database: {e}"))?;
    let n = storage::recover_streaming(&db.lock()).map_err(|e| e.to_string())?;
    if n > 0 {
        info!("marked {n} interrupted generation(s) as cancelled");
    }
    Ok(app::App::new(paths, config, secrets, db))
}

/// Move the pre-skill Google settings into `[skills.google-workspace]`.
fn migrate_google(paths: &Paths, config: &mut Config, secrets: &mut Secrets) -> Result<(), String> {
    const ID: &str = "google-workspace";
    let old = std::mem::take(&mut config.tools.google);
    let old_secret = secrets.get("google.client_secret").map(str::to_string);
    let old_refresh = secrets.get("google.refresh_token").map(str::to_string);
    if old.is_empty() && old_secret.is_none() && old_refresh.is_none() {
        return Ok(());
    }
    let st = config.skills.entry(ID.into()).or_default();
    if !old.client_id.is_empty() {
        st.settings.entry("client_id".into()).or_insert(old.client_id);
    }
    if !old.services.is_empty() && st.groups.is_none() {
        st.groups = Some(old.services);
    }
    if !old.api_base.is_empty() {
        let b = old.api_base.trim_end_matches('/').to_string();
        st.settings.insert("api_base".into(), b.clone());
        st.settings.insert("authorize_url".into(), format!("{b}/auth"));
        st.settings.insert("token_url".into(), format!("{b}/oauth/token"));
        st.settings.insert("revoke_url".into(), format!("{b}/oauth/revoke"));
    }
    for (old_key, new_key) in [("google.client_secret", "client_secret"), ("google.refresh_token", "refresh_token")] {
        if let Some(v) = secrets.get(old_key).map(str::to_string) {
            secrets.set(&skills::secret_key(ID, new_key), &v);
            secrets.set(old_key, "");
        }
    }
    config.save(paths)?;
    secrets.save(paths)?;
    info!("moved Google settings into the google-workspace skill");
    Ok(())
}

pub async fn serve(paths: Paths) -> Result<(), String> {
    let app = build_app(paths)?;
    channels::start(&app);
    schedule::start(&app);
    providers::catalog::load_cached(&app);
    providers::catalog::refresh_if_stale(&app);
    // Start Codex's app-server now so the first reply does not wait for it.
    if std::env::var_os("ALICE_CODEX_EXEC").is_none() {
        for p in app.config().providers.iter().filter(|p| p.kind == config::ProviderKind::CodexCli) {
            providers::codex_server::warm_up(p.base_url().to_string());
        }
    }
    server::serve(app).await.map_err(|e| e.to_string())
}
