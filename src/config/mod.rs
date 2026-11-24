//! Application settings (config.toml) and credentials (secrets.toml).
//!
//! Neither file ever contains conversation-derived data. Deleting all
//! threads leaves both untouched.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    /// `$ALICE_HOME` or `~/.alice`.
    pub fn resolve() -> Paths {
        if let Some(h) = std::env::var_os("ALICE_HOME") {
            return Paths { home: PathBuf::from(h) };
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        Paths { home: home.join(".alice") }
    }
    pub fn at(home: impl Into<PathBuf>) -> Paths {
        Paths { home: home.into() }
    }
    pub fn config(&self) -> PathBuf { self.home.join("config.toml") }
    pub fn secrets(&self) -> PathBuf { self.home.join("secrets.toml") }
    pub fn db(&self) -> PathBuf { self.home.join("alice.db") }
    pub fn attachments(&self) -> PathBuf { self.home.join("attachments") }
    pub fn workspace(&self) -> PathBuf { self.home.join("workspace") }
    pub fn logs(&self) -> PathBuf { self.home.join("logs") }
    pub fn log_file(&self) -> PathBuf { self.logs().join("alice.log") }
    pub fn pid_file(&self) -> PathBuf { self.home.join("alice.pid") }

    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [self.home.clone(), self.attachments(), self.workspace(), self.logs()] {
            std::fs::create_dir_all(&d)?;
            set_mode(&d, 0o700);
        }
        Ok(())
    }
}

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
}
#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Config {
    pub server: ServerConfig,
    pub agent: AgentConfig,
    pub memory: MemoryConfig,
    #[serde(rename = "providers")]
    pub providers: Vec<ProviderConfig>,
    pub tools: ToolsConfig,
    pub channels: ChannelsConfig,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind: String,
    pub port: u16,
    /// URL other devices use to reach this server (Tailscale / reverse proxy).
    pub public_url: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig { bind: "127.0.0.1".into(), port: 7878, public_url: String::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub name: String,
    /// System prompt set explicitly by the user (kept on full reset).
    pub system_prompt: String,
    pub language: String,
    /// `provider_id/model_name`
    pub default_model: String,
    pub max_steps: usize,
    /// Approximate character budget for thread history sent to the model.
    pub history_chars: usize,
    pub max_tokens: u32,
    pub appearance: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            name: "Alice".into(),
            system_prompt: String::new(),
            language: "ja".into(),
            default_model: String::new(),
            max_steps: 16,
            history_chars: 60_000,
            max_tokens: 16_000,
            appearance: "system".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub auto_extract: bool,
    /// Model used for extraction; empty = thread's model.
    pub model: String,
    pub inject_limit: usize,
    /// User messages shorter than this (chars) are not sent for extraction.
    pub min_chars: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig { auto_extract: true, model: String::new(), inject_limit: 8, min_chars: 8 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Openai,
    OpenaiCompatible,
    Ollama,
    Anthropic,
    Gemini,
    CodexCli,
    GeminiCli,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 7] = [
        ProviderKind::Openai,
        ProviderKind::Anthropic,
        ProviderKind::Gemini,
        ProviderKind::OpenaiCompatible,
        ProviderKind::Ollama,
        ProviderKind::CodexCli,
        ProviderKind::GeminiCli,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Openai => "openai",
            ProviderKind::OpenaiCompatible => "openai-compatible",
            ProviderKind::Ollama => "ollama",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
            ProviderKind::CodexCli => "codex-cli",
            ProviderKind::GeminiCli => "gemini-cli",
        }
    }
    pub fn parse(s: &str) -> Option<ProviderKind> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::Openai => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com",
            ProviderKind::Gemini => "https://generativelanguage.googleapis.com",
            ProviderKind::Ollama => "http://127.0.0.1:11434/v1",
            ProviderKind::OpenaiCompatible => "",
            ProviderKind::CodexCli => "codex",
            ProviderKind::GeminiCli => "gemini",
        }
    }
    pub fn needs_api_key(self) -> bool {
        matches!(self, ProviderKind::Openai | ProviderKind::Anthropic | ProviderKind::Gemini)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    /// HTTP base URL, or the executable for CLI-based providers.
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<String>,
    /// Context window in tokens (used for history budgeting); 0 = default.
    #[serde(default)]
    pub context_tokens: u32,
}

impl ProviderConfig {
    pub fn base_url(&self) -> &str {
        if self.base_url.is_empty() { self.kind.default_base_url() } else { &self.base_url }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Disabled,
    Ask,
    Allow,
}

impl Permission {
    pub fn parse(s: &str) -> Option<Permission> {
        match s {
            "disabled" | "off" => Some(Permission::Disabled),
            "ask" => Some(Permission::Ask),
            "allow" | "always" => Some(Permission::Allow),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::Disabled => "disabled",
            Permission::Ask => "ask",
            Permission::Allow => "allow",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub fs: FsConfig,
    pub web: WebConfig,
    pub google: GoogleConfig,
    /// Per-tool overrides of the built-in default permission.
    pub permissions: BTreeMap<String, Permission>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FsConfig {
    /// Directories the file tools may access. Empty = `~/.alice/workspace` only.
    pub roots: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    /// "brave" | "searxng" | "" (search disabled)
    pub search_provider: String,
    pub searxng_url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleConfig {
    pub client_id: String,
    /// Enabled service groups: gmail, calendar, drive, docs, sheets
    pub services: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelsConfig {
    pub telegram: TelegramConfig,
    pub discord: DiscordConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub allowed_user_ids: Vec<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub allowed_user_ids: Vec<String>,
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Config, String> {
        match std::fs::read_to_string(paths.config()) {
            Ok(s) => toml::from_str(&s).map_err(|e| format!("config.toml: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("config.toml: {e}")),
        }
    }

    pub fn save(&self, paths: &Paths) -> Result<(), String> {
        let s = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(&paths.config(), s.as_bytes(), 0o600)
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Resolve `provider/model` into its provider config and model name.
    pub fn resolve_model<'a>(&'a self, model_id: &'a str) -> Option<(&'a ProviderConfig, &'a str)> {
        let (pid, name) = model_id.split_once('/')?;
        let p = self.provider(pid)?;
        Some((p, name))
    }

    pub fn all_models(&self) -> Vec<String> {
        self.providers
            .iter()
            .flat_map(|p| p.models.iter().map(move |m| format!("{}/{}", p.id, m)))
            .collect()
    }

    /// Get a value by dotted key, as a TOML-ish string.
    pub fn get_key(&self, key: &str) -> Option<String> {
        let v = toml::Value::try_from(self).ok()?;
        let mut cur = &v;
        for part in key.split('.') {
            cur = cur.get(part)?;
        }
        Some(match cur {
            toml::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }

    /// Set a value by dotted key. The value is parsed as TOML, falling back to a string.
    pub fn set_key(&mut self, key: &str, raw: &str) -> Result<(), String> {
        if key.starts_with("providers") {
            return Err("use `alice model add/remove` to change providers".into());
        }
        let mut root = toml::Value::try_from(&*self).map_err(|e| e.to_string())?;
        let parts: Vec<&str> = key.split('.').collect();
        let mut cur = &mut root;
        for part in &parts[..parts.len() - 1] {
            cur = cur
                .get_mut(*part)
                .ok_or_else(|| format!("unknown key: {key}"))?;
        }
        let last = parts[parts.len() - 1];
        let table = cur.as_table_mut().ok_or_else(|| format!("unknown key: {key}"))?;
        let existing = table.get(last).ok_or_else(|| format!("unknown key: {key}"))?;
        let parsed = match existing {
            toml::Value::String(_) => toml::Value::String(raw.to_string()),
            _ => toml::from_str::<toml::Table>(&format!("v = {raw}"))
                .ok()
                .and_then(|mut t| t.remove("v"))
                .ok_or_else(|| format!("invalid value for {key}: {raw}"))?,
        };
        if std::mem::discriminant(existing) != std::mem::discriminant(&parsed) {
            return Err(format!("type mismatch for {key}"));
        }
        table.insert(last.to_string(), parsed);
        *self = root.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        Ok(())
    }
}

/// Credentials, stored separately with 0600 permissions. Flat `name = "value"` table.
/// Names: `server.token`, `provider.<id>.api_key`, `telegram.bot_token`,
/// `discord.bot_token`, `google.client_secret`, `google.refresh_token`, `brave.api_key`.
#[derive(Clone, Debug, Default)]
pub struct Secrets {
    map: BTreeMap<String, String>,
}

impl Secrets {
    pub fn load(paths: &Paths) -> Result<Secrets, String> {
        match std::fs::read_to_string(paths.secrets()) {
            Ok(s) => {
                let map: BTreeMap<String, String> =
                    toml::from_str(&s).map_err(|e| format!("secrets.toml: {e}"))?;
                Ok(Secrets { map })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Secrets::default()),
            Err(e) => Err(format!("secrets.toml: {e}")),
        }
    }
    pub fn save(&self, paths: &Paths) -> Result<(), String> {
        let s = toml::to_string(&self.map).map_err(|e| e.to_string())?;
        write_atomic(&paths.secrets(), s.as_bytes(), 0o600)
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.map.get(name).map(|s| s.as_str()).filter(|s| !s.is_empty())
    }
    pub fn set(&mut self, name: &str, value: &str) {
        if value.is_empty() {
            self.map.remove(name);
        } else {
            self.map.insert(name.to_string(), value.to_string());
        }
    }
    pub fn remove_prefix(&mut self, prefix: &str) {
        self.map.retain(|k, _| !k.starts_with(prefix));
    }
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }
    pub fn provider_key(&self, provider_id: &str) -> Option<&str> {
        self.get(&format!("provider.{provider_id}.api_key"))
    }
}

pub fn write_atomic(path: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    use std::io::Write;
    let dir = path.parent().ok_or("invalid path")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(mode);
    }
    let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(data).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    drop(f);
    set_mode(&tmp, mode);
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_and_set_keys() {
        let mut c = Config::default();
        c.set_key("server.port", "9000").unwrap();
        assert_eq!(c.server.port, 9000);
        c.set_key("agent.name", "Alicia").unwrap();
        assert_eq!(c.get_key("agent.name").unwrap(), "Alicia");
        c.set_key("memory.auto_extract", "false").unwrap();
        assert!(!c.memory.auto_extract);
        assert!(c.set_key("server.port", "abc").is_err());
        assert!(c.set_key("nope.x", "1").is_err());
    }

    #[test]
    fn roundtrip() {
        let mut c = Config::default();
        c.providers.push(ProviderConfig {
            id: "openai".into(),
            kind: ProviderKind::Openai,
            base_url: String::new(),
            models: vec!["gpt-x".into()],
            context_tokens: 0,
        });
        let s = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.all_models(), vec!["openai/gpt-x".to_string()]);
        assert_eq!(back.resolve_model("openai/gpt-x").unwrap().1, "gpt-x");
    }
}
