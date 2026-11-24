//! Process-wide state shared by the HTTP server, the agent and channels.

use crate::config::{Config, Paths, Secrets};
use crate::storage::Db;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use tokio::sync::{broadcast, oneshot};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct Event {
    pub thread_id: Option<String>,
    pub json: Arc<str>,
}

pub struct ActiveGeneration {
    pub cancel: CancellationToken,
    pub client: String,
}

/// A Codex (or other MCP client) turn that may call tools through `alice mcp`.
/// Keyed by a random per-turn token; removed when the turn ends.
pub struct Bridge {
    pub thread_id: String,
    pub message_id: String,
    pub client: String,
    pub cancel: CancellationToken,
    /// Calls made so far (recorded on the assistant message).
    pub calls: Mutex<Vec<crate::providers::ToolCall>>,
}

pub struct PendingApproval {
    pub thread_id: String,
    pub reply: oneshot::Sender<bool>,
}

pub struct App {
    pub paths: Paths,
    pub config: RwLock<Config>,
    pub secrets: RwLock<Secrets>,
    pub db: Db,
    pub http: reqwest::Client,
    pub events: broadcast::Sender<Event>,
    pub active: Mutex<HashMap<String, ActiveGeneration>>,
    pub approvals: Mutex<HashMap<String, PendingApproval>>,
    /// Live gateway state for the settings UI: name -> {state, detail}.
    pub channel_status: Mutex<HashMap<&'static str, Value>>,
    pub started: Instant,
    pub skill_cache: Mutex<Option<(crate::skills::Fingerprint, Arc<crate::skills::Catalog>)>>,
    pub auth: crate::skills::auth::AuthState,
    pub bridges: Mutex<HashMap<String, Arc<Bridge>>>,
}

pub type AppRef = Arc<App>;

impl App {
    pub fn new(paths: Paths, config: Config, secrets: Secrets, db: Db) -> AppRef {
        let (events, _) = broadcast::channel(1024);
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .user_agent(concat!("alice-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client");
        Arc::new(App {
            paths,
            config: RwLock::new(config),
            secrets: RwLock::new(secrets),
            db,
            http,
            events,
            active: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            channel_status: Mutex::new(HashMap::new()),
            started: Instant::now(),
            skill_cache: Mutex::new(None),
            auth: Default::default(),
            bridges: Mutex::new(HashMap::new()),
        })
    }

    pub fn config(&self) -> Config {
        self.config.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn secrets(&self) -> Secrets {
        self.secrets.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Mutate and persist config.toml.
    pub fn update_config(&self, f: impl FnOnce(&mut Config) -> Result<(), String>) -> Result<Config, String> {
        let mut guard = self.config.write().unwrap_or_else(|e| e.into_inner());
        let mut next = guard.clone();
        f(&mut next)?;
        next.save(&self.paths)?;
        *guard = next.clone();
        Ok(next)
    }

    pub fn update_secrets(&self, f: impl FnOnce(&mut Secrets)) -> Result<(), String> {
        let mut guard = self.secrets.write().unwrap_or_else(|e| e.into_inner());
        let mut next = guard.clone();
        f(&mut next);
        next.save(&self.paths)?;
        *guard = next;
        Ok(())
    }

    /// Broadcast an event to all SSE subscribers and channel tasks.
    pub fn emit(&self, mut v: Value) {
        let thread_id = v.get("thread_id").and_then(|t| t.as_str()).map(str::to_string);
        if v.get("ts").is_none() {
            v["ts"] = Value::from(crate::util::now_ms());
        }
        let _ = self.events.send(Event { thread_id, json: v.to_string().into() });
    }

    pub fn is_generating(&self, thread_id: &str) -> bool {
        self.active.lock().unwrap_or_else(|e| e.into_inner()).contains_key(thread_id)
    }

    /// Cancel the generation running in a thread, if any. Returns true if one was running.
    pub fn cancel_generation(&self, thread_id: &str) -> bool {
        let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        match active.get(thread_id) {
            Some(g) => {
                g.cancel.cancel();
                true
            }
            None => false,
        }
    }

    pub fn set_channel_status(&self, name: &'static str, state: &str, detail: &str) {
        self.channel_status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name, serde_json::json!({ "state": state, "detail": detail }));
    }

    /// Open a bridge for one turn; returns its token. Close with [`App::close_bridge`].
    pub fn open_bridge(&self, b: Bridge) -> (String, Arc<Bridge>) {
        let token = crate::util::new_token() + &crate::util::new_token();
        let b = Arc::new(b);
        self.bridges.lock().unwrap_or_else(|e| e.into_inner()).insert(token.clone(), b.clone());
        (token, b)
    }

    pub fn close_bridge(&self, token: &str) {
        self.bridges.lock().unwrap_or_else(|e| e.into_inner()).remove(token);
    }

    /// Constant-time lookup of a bridge token.
    pub fn bridge(&self, token: &str) -> Option<Arc<Bridge>> {
        let map = self.bridges.lock().unwrap_or_else(|e| e.into_inner());
        map.iter().find(|(k, _)| crate::util::ct_eq(k.as_bytes(), token.as_bytes())).map(|(_, b)| b.clone()).filter(|b| !b.cancel.is_cancelled())
    }

    /// Reject all pending approvals belonging to a thread.
    pub fn drop_approvals(&self, thread_id: Option<&str>) {
        let mut ap = self.approvals.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<String> = ap
            .iter()
            .filter(|(_, p)| thread_id.is_none_or(|t| p.thread_id == t))
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            if let Some(p) = ap.remove(&id) {
                let _ = p.reply.send(false);
            }
        }
    }
}
