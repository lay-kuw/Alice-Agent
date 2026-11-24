//! Built-in tools and the permission gate.
//!
//! Permissions live only in config.toml. No tool or API reachable by the
//! model can change them.

pub mod fs;
pub mod google;
pub mod web;

use crate::app::App;
use crate::config::{Config, Permission};
use crate::providers::ToolSpec;
use serde_json::{Value, json};

pub struct ToolDef {
    pub name: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    /// Default permission when the user has not configured one.
    pub default: Permission,
    pub params: fn() -> Value,
}

pub fn all() -> Vec<&'static ToolDef> {
    fs::DEFS.iter().chain(web::DEFS.iter()).chain(google::DEFS.iter()).collect()
}

pub fn find(name: &str) -> Option<&'static ToolDef> {
    all().into_iter().find(|d| d.name == name)
}

/// Whether the tool's group is configured at all (e.g. a search provider set).
pub fn available(cfg: &Config, app: &App, def: &ToolDef) -> bool {
    match def.group {
        "web" => web::available(cfg, app, def.name),
        "google" => google::available(cfg, app, def.name),
        _ => true,
    }
}

/// Effective permission: per-tool override, then group override, then default.
pub fn permission(cfg: &Config, def: &ToolDef) -> Permission {
    cfg.tools
        .permissions
        .get(def.name)
        .or_else(|| cfg.tools.permissions.get(def.group))
        .copied()
        .unwrap_or(def.default)
}

/// Tool specs offered to the model: enabled and available tools only.
pub fn specs(cfg: &Config, app: &App) -> Vec<ToolSpec> {
    all()
        .into_iter()
        .filter(|d| permission(cfg, d) != Permission::Disabled && available(cfg, app, d))
        .map(|d| ToolSpec { name: d.name.into(), description: d.description.into(), parameters: (d.params)() })
        .collect()
}

pub fn list_json(cfg: &Config, app: &App) -> Value {
    Value::Array(
        all()
            .into_iter()
            .map(|d| {
                json!({
                    "name": d.name, "group": d.group, "description": d.description,
                    "permission": permission(cfg, d).as_str(),
                    "default": d.default.as_str(),
                    "available": available(cfg, app, d),
                })
            })
            .collect(),
    )
}

/// Execute a tool. The caller has already checked permission.
pub async fn execute(app: &App, name: &str, args: &Value) -> Result<String, String> {
    let cfg = app.config();
    match find(name).map(|d| d.group) {
        Some("fs") => fs::execute(&cfg, &app.paths, name, args).await,
        Some("web") => web::execute(app, &cfg, name, args).await,
        Some("google") => google::execute(app, &cfg, name, args).await,
        _ => Err(format!("unknown tool: {name}")),
    }
}

/// Wrap external content so the model treats it as data.
pub fn untrusted(source: &str, body: &str) -> String {
    let safe = body.replace("</untrusted>", "</ untrusted>");
    format!("<untrusted source=\"{}\">\n{}\n</untrusted>", source.replace('"', "'"), safe)
}

pub(crate) fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| format!("missing argument: {key}"))
}
