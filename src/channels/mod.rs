//! Telegram / Discord gateways (Phase 5).

use crate::app::AppRef;
use crate::config::Paths;

/// Start enabled channel tasks inside the server process.
pub fn start(_app: &AppRef) {}

pub async fn gateway_cmd(_paths: &Paths, _args: Vec<String>) -> Result<(), String> {
    Err("Telegram / Discord gateways are not implemented yet (roadmap Phase 5)".into())
}
