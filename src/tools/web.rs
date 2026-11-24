//! Web tools (Phase 4).

use super::ToolDef;
use crate::app::App;
use crate::config::Config;
use serde_json::Value;

pub static DEFS: [ToolDef; 0] = [];

pub fn available(_cfg: &Config, _app: &App, _name: &str) -> bool {
    false
}

pub async fn execute(_app: &App, _cfg: &Config, name: &str, _args: &Value) -> Result<String, String> {
    Err(format!("unknown tool: {name}"))
}
