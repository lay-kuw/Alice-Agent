//! Model discovery. Each provider that can list its models (Codex via
//! `app-server`, Gemini / Anthropic / OpenAI-compatible servers via their
//! APIs) is asked for them; the ids are written to the provider's `models`
//! in config.toml (so validation stays a plain config lookup) and names,
//! descriptions and reasoning efforts are kept in memory for the pickers.

use crate::app::App;
use crate::config::{ProviderConfig, ProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    pub model: String,
    pub name: String,
    pub description: String,
    /// Reasoning efforts the model accepts (`provider/model#effort`).
    #[serde(default)]
    pub efforts: Vec<String>,
    #[serde(default)]
    pub default_effort: Option<String>,
    /// What the provider uses when no model is given (Codex "default").
    #[serde(default)]
    pub is_default: bool,
}

/// Discovered models per provider id, with when they were fetched.
pub type Discovered = std::collections::HashMap<String, (Instant, Vec<ModelInfo>)>;

const STALE: Duration = Duration::from_secs(6 * 3600);

fn info(model: &str, name: &str, description: &str) -> ModelInfo {
    ModelInfo { model: model.into(), name: name.into(), description: description.into(), efforts: vec![], default_effort: None, is_default: false }
}

/// Ask one provider for its models. `Ok(None)`: the provider cannot list them.
pub async fn discover(http: &reqwest::Client, p: &ProviderConfig, key: Option<&str>) -> Result<Option<Vec<ModelInfo>>, String> {
    let base = p.base_url().trim_end_matches('/').to_string();
    let get = |url: String| http.get(url).timeout(Duration::from_secs(20));
    let json = |r: reqwest::Response| async move {
        if !r.status().is_success() {
            return Err(format!("HTTP {}", r.status().as_u16()));
        }
        r.json::<Value>().await.map_err(|e| e.without_url().to_string())
    };
    let models = match p.kind {
        ProviderKind::CodexCli => super::cli::codex_models(&base)
            .await?
            .iter()
            .map(|m| ModelInfo {
                model: m["model"].as_str().or(m["id"].as_str()).unwrap_or("").into(),
                name: m["displayName"].as_str().unwrap_or("").into(),
                description: m["description"].as_str().unwrap_or("").into(),
                efforts: m["supportedReasoningEfforts"].as_array().into_iter().flatten().filter_map(|e| e["reasoningEffort"].as_str().map(str::to_string)).collect(),
                default_effort: m["defaultReasoningEffort"].as_str().map(str::to_string),
                is_default: m["isDefault"] == true,
            })
            .collect(),
        ProviderKind::Gemini => {
            let Some(key) = key else { return Err("API key is not set".into()) };
            let r = get(format!("{base}/v1beta/models?pageSize=200")).header("x-goog-api-key", key).send().await.map_err(|e| e.without_url().to_string())?;
            let v = json(r).await?;
            v["models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| m["supportedGenerationMethods"].as_array().is_some_and(|a| a.iter().any(|x| x == "generateContent")))
                .filter_map(|m| {
                    let id = m["name"].as_str()?.strip_prefix("models/")?;
                    // Chat models only.
                    let skip = ["embedding", "aqa", "tts", "image", "live", "veo", "imagen"];
                    (!skip.iter().any(|s| id.contains(s))).then(|| info(id, m["displayName"].as_str().unwrap_or(id), m["description"].as_str().unwrap_or("")))
                })
                .collect()
        }
        ProviderKind::Anthropic => {
            let Some(key) = key else { return Err("API key is not set".into()) };
            let r = get(format!("{base}/v1/models?limit=100")).header("x-api-key", key).header("anthropic-version", "2023-06-01").send().await.map_err(|e| e.without_url().to_string())?;
            let v = json(r).await?;
            v["data"].as_array().into_iter().flatten().filter_map(|m| Some(info(m["id"].as_str()?, m["display_name"].as_str().unwrap_or(""), ""))).collect()
        }
        ProviderKind::OpenaiCompatible | ProviderKind::Ollama => {
            let mut r = get(format!("{base}/models"));
            if let Some(k) = key {
                r = r.bearer_auth(k);
            }
            let v = json(r.send().await.map_err(|e| e.without_url().to_string())?).await?;
            v["data"].as_array().into_iter().flatten().filter_map(|m| Some(info(m["id"].as_str()?, "", ""))).collect()
        }
        // OpenAI's list mixes in audio, image and embedding models; Gemini CLI has no list.
        ProviderKind::Openai | ProviderKind::GeminiCli => return Ok(None),
    };
    Ok(Some(models).filter(|m: &Vec<ModelInfo>| !m.is_empty()))
}

/// Refresh every provider (in parallel); update config and the in-memory catalog.
/// Returns per-provider results for the UI.
pub async fn refresh(app: &App) -> Vec<Value> {
    let cfg = app.config();
    let secrets = app.secrets();
    let jobs = cfg.providers.iter().map(|p| {
        let key = secrets.provider_key(&p.id).map(str::to_string);
        async move { (p.id.clone(), p.kind, discover(&app.http, p, key.as_deref()).await) }
    });
    let results = futures_util::future::join_all(jobs).await;
    let mut report = Vec::new();
    let mut found: Vec<(String, ProviderKind, Vec<ModelInfo>)> = Vec::new();
    for (id, kind, r) in results {
        match r {
            Ok(Some(list)) => {
                report.push(json!({ "id": id, "ok": true, "models": list.len() }));
                found.push((id, kind, list));
            }
            Ok(None) => report.push(json!({ "id": id, "ok": true, "models": null })),
            Err(e) => {
                crate::warn!("model list for {id} failed: {e}");
                // Remember the attempt (keeping what was known) so it is not retried on every request.
                let mut d = app.models.lock().unwrap_or_else(|e| e.into_inner());
                let known = d.remove(&id).map(|(_, l)| l).unwrap_or_default();
                d.insert(id.clone(), (Instant::now(), known));
                report.push(json!({ "id": id, "ok": false, "error": e }));
            }
        }
    }
    if found.is_empty() {
        return report;
    }
    {
        let mut d = app.models.lock().unwrap_or_else(|e| e.into_inner());
        for (id, _, list) in &found {
            d.insert(id.clone(), (Instant::now(), list.clone()));
        }
        // Names and efforts are there right after a restart (not secret).
        let snapshot: std::collections::BTreeMap<&String, &Vec<ModelInfo>> = d.iter().map(|(k, (_, l))| (k, l)).collect();
        if let Ok(s) = serde_json::to_vec_pretty(&snapshot) {
            let _ = crate::config::write_atomic(&cache_file(app), &s, 0o600);
        }
    }
    let _ = app.update_config(|c| {
        let default = c.agent.default_model.clone();
        for (id, kind, list) in &found {
            let Some(p) = c.providers.iter_mut().find(|p| &p.id == id) else { continue };
            let mut models: Vec<String> = Vec::new();
            // CLIs keep "default" (the CLI's own choice).
            if matches!(kind, ProviderKind::CodexCli | ProviderKind::GeminiCli) {
                models.push("default".into());
            }
            models.extend(list.iter().map(|m| m.model.clone()));
            // Never drop the model that is the default.
            if let Some(name) = crate::config::split_effort(&default).0.strip_prefix(&format!("{id}/"))
                && !models.iter().any(|m| m == name) {
                    models.push(name.to_string());
                }
            p.models = models;
        }
        Ok(())
    });
    // Open pages reload their model pickers.
    app.emit(json!({ "type": "models.updated" }));
    report
}

fn cache_file(app: &App) -> std::path::PathBuf {
    app.paths.home.join("models.json")
}

/// Load the last discovered catalog (marked stale, so it is refreshed soon).
pub fn load_cached(app: &App) {
    let Ok(bytes) = std::fs::read(cache_file(app)) else { return };
    let Ok(map) = serde_json::from_slice::<std::collections::HashMap<String, Vec<ModelInfo>>>(&bytes) else { return };
    let old = Instant::now().checked_sub(STALE + Duration::from_secs(1)).unwrap_or_else(Instant::now);
    let mut d = app.models.lock().unwrap_or_else(|e| e.into_inner());
    for (id, list) in map {
        d.entry(id).or_insert((old, list));
    }
}

/// Refresh in the background when nothing was discovered yet or it is old.
pub fn refresh_if_stale(app: &crate::app::AppRef) {
    let stale = {
        let d = app.models.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = app.config();
        cfg.providers.iter().any(|p| !matches!(p.kind, ProviderKind::Openai | ProviderKind::GeminiCli) && d.get(&p.id).is_none_or(|(t, _)| t.elapsed() > STALE))
    };
    if stale && !app.models_refreshing.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let app = app.clone();
        tokio::spawn(async move {
            refresh(&app).await;
            app.models_refreshing.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }
}

/// Every registered model with what is known about it, for the pickers.
pub fn catalog(app: &App) -> Vec<Value> {
    let cfg = app.config();
    let d = app.models.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = Vec::new();
    for p in &cfg.providers {
        let known = d.get(&p.id).map(|(_, l)| l.as_slice()).unwrap_or(&[]);
        for m in &p.models {
            // "default" behaves like the provider's default model.
            let i = if m == "default" { known.iter().find(|k| k.is_default) } else { known.iter().find(|k| &k.model == m) };
            let name = match (i, m.as_str()) {
                (Some(i), "default") => format!("Default ({})", if i.name.is_empty() { &i.model } else { &i.name }),
                (_, "default") => "Default".into(),
                (Some(i), _) if !i.name.is_empty() => i.name.clone(),
                _ => m.clone(),
            };
            out.push(json!({
                "id": format!("{}/{m}", p.id), "provider": p.id, "kind": p.kind.as_str(), "model": m, "name": name,
                "description": i.map(|i| i.description.as_str()).unwrap_or(""),
                "efforts": i.map(|i| i.efforts.clone()).unwrap_or_default(),
                "default_effort": i.and_then(|i| i.default_effort.clone()),
                // The concrete model behind "default", when known.
                "resolved": i.filter(|_| m == "default").map(|i| i.model.clone()),
            }));
        }
    }
    out
}

/// A model id is usable: registered, and its effort (if any) is one the model
/// lists. Unknown effort lists (not yet discovered) are not second-guessed.
pub fn validate(app: &App, model_id: &str) -> Result<(), String> {
    let cfg = app.config();
    let Some((p, name)) = cfg.resolve_model(model_id) else { return Err(format!("unknown model: {model_id}")) };
    if let Some(e) = crate::config::split_effort(model_id).1 {
        let d = app.models.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = d.get(&p.id).and_then(|(_, l)| l.iter().find(|m| if name == "default" { m.is_default } else { m.model == name }))
            && !m.efforts.is_empty() && !m.efforts.iter().any(|x| x == e) {
                return Err(format!("{name} does not support reasoning effort '{e}' (choose: {})", m.efforts.join(", ")));
            }
    }
    Ok(())
}

/// "GPT-6.2-Sol · high" for a model id (resolving "default" and the effort),
/// or the id itself when nothing is known.
pub fn display_name(app: &App, model_id: &str) -> String {
    let (base, effort) = crate::config::split_effort(model_id);
    let cat = catalog(app);
    let Some(i) = cat.iter().find(|c| c["id"] == base) else { return model_id.to_string() };
    let resolved = i["resolved"].as_str();
    let name = match resolved {
        // "Default (GPT-6.2-Sol)" -> "GPT-6.2-Sol"
        Some(r) => cat.iter().find(|c| c["provider"] == i["provider"] && c["model"] == r).and_then(|c| c["name"].as_str()).unwrap_or(r).to_string(),
        None => i["name"].as_str().unwrap_or(base).to_string(),
    };
    match effort.or(i["default_effort"].as_str()) {
        Some(e) => format!("{name} · {e}"),
        None => name,
    }
}
