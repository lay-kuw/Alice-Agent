//! Skill system: manifests, discovery (Skill Manager) and the Tool Registry.
//!
//! A skill is a folder with `skill.toml` (manifest) and `SKILL.md` (how the
//! model should use it). Built-in skills are compiled in; user skills live in
//! `~/.alice/skills/<id>/` and are picked up automatically. Every tool call,
//! whatever the model or transport, goes through [`gate`].

pub mod auth;
pub mod gate;
pub mod handlers;

use crate::app::App;
use crate::config::{Config, Permission};
use crate::providers::ToolSpec;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Built-in skills: (id, skill.toml, SKILL.md).
const BUNDLED: [(&str, &str, &str); 3] = [
    ("files", include_str!("../../skills/files/skill.toml"), include_str!("../../skills/files/SKILL.md")),
    ("web", include_str!("../../skills/web/skill.toml"), include_str!("../../skills/web/SKILL.md")),
    (
        "google-workspace",
        include_str!("../../skills/google-workspace/skill.toml"),
        include_str!("../../skills/google-workspace/SKILL.md"),
    ),
];

/// Tool that returns a skill's SKILL.md. Always offered when any skill is usable.
pub const GUIDE_TOOL: &str = "skill_guide";

// ------------------------------------------------------------------ manifest

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub description: String,
    /// Older names accepted in `tools.permissions` (e.g. "fs" for files).
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Built-in implementation module for `handler = "builtin"` tools.
    #[serde(default)]
    pub builtin: Option<String>,
    #[serde(default)]
    pub requires: Requires,
    #[serde(default)]
    pub permissions: Declared,
    #[serde(default)]
    pub auth: Option<AuthSpec>,
    #[serde(default)]
    pub settings: BTreeMap<String, SettingSpec>,
    #[serde(default)]
    pub groups: Vec<GroupSpec>,
    #[serde(default)]
    pub tools: Vec<ToolManifest>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    #[serde(default)]
    pub bins: Vec<String>,
    #[serde(default)]
    pub settings: Vec<String>,
    #[serde(default)]
    pub auth: bool,
}

/// What the skill declares it needs; shown before enabling. `network` is
/// enforced for `http` tools.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub files: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSpec {
    /// "token" | "oauth2"
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub authorize_url: String,
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub revoke_url: String,
    #[serde(default = "yes")]
    pub pkce: bool,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingSpec {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub default: Option<String>,
    /// Hidden behind "advanced" in the UI.
    #[serde(default)]
    pub advanced: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupSpec {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub risk: Option<String>,
    /// "builtin" | "http" | "command"
    pub handler: String,
    /// JSON Schema, as a TOML table or a JSON string.
    #[serde(default)]
    pub input_schema: Option<toml::Value>,
    // http
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub query: Vec<String>,
    #[serde(default)]
    pub body: Vec<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    // command
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl ToolManifest {
    pub fn schema(&self) -> Result<Value, String> {
        match &self.input_schema {
            None => Ok(json!({ "type": "object", "properties": {} })),
            Some(toml::Value::String(s)) => serde_json::from_str(s).map_err(|e| format!("tool {}: input_schema: {e}", self.name)),
            Some(v) => serde_json::to_value(v).map_err(|e| e.to_string()),
        }
    }
    pub fn default_permission(&self) -> Permission {
        self.permission.as_deref().and_then(Permission::parse).unwrap_or(Permission::Ask)
    }
    pub fn risk(&self) -> &str {
        self.risk.as_deref().unwrap_or("write")
    }
}

fn valid_skill_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 48 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_tool_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let m: Manifest = toml::from_str(text).map_err(|e| e.to_string())?;
    if !valid_skill_id(&m.id) {
        return Err(format!("invalid skill id '{}' (use a-z, 0-9 and -)", m.id));
    }
    if let Some(b) = &m.builtin
        && !matches!(b.as_str(), "fs" | "web" | "google") {
            return Err(format!("unknown builtin module '{b}'"));
        }
    if let Some(a) = &m.auth
        && !matches!(a.kind.as_str(), "none" | "token" | "oauth2") {
            return Err(format!("unknown auth type '{}'", a.kind));
        }
    if m.auth.as_ref().is_some_and(|a| a.kind == "oauth2" && (a.authorize_url.is_empty() || a.token_url.is_empty())) {
        return Err("oauth2 auth needs authorize_url and token_url".into());
    }
    let mut seen = std::collections::HashSet::new();
    for t in &m.tools {
        if !valid_tool_name(&t.name) || t.name == GUIDE_TOOL {
            return Err(format!("invalid tool name '{}' (use a-z, 0-9 and _)", t.name));
        }
        if !seen.insert(t.name.as_str()) {
            return Err(format!("duplicate tool '{}'", t.name));
        }
        t.schema()?;
        if let Some(g) = &t.group
            && !m.groups.iter().any(|x| &x.id == g) {
                return Err(format!("tool {}: unknown group '{g}'", t.name));
            }
        if t.permission.as_deref().is_some_and(|p| Permission::parse(p).is_none()) {
            return Err(format!("tool {}: invalid permission", t.name));
        }
        match t.handler.as_str() {
            "builtin" if m.builtin.is_none() => return Err(format!("tool {}: handler builtin needs `builtin = \"…\"`", t.name)),
            "builtin" => {}
            "http" if t.url.is_none() => return Err(format!("tool {}: http handler needs url", t.name)),
            "http" => {
                let method = t.method.as_deref().unwrap_or("GET");
                if !matches!(method, "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
                    return Err(format!("tool {}: unsupported method {method}", t.name));
                }
            }
            "command" if t.command.is_empty() => return Err(format!("tool {}: command handler needs command", t.name)),
            "command" => {}
            h => return Err(format!("tool {}: unknown handler '{h}'", t.name)),
        }
    }
    Ok(m)
}

// ------------------------------------------------------------------ skills

#[derive(Clone, Debug)]
pub struct Skill {
    pub manifest: Manifest,
    pub guide: String,
    /// `None` for built-in skills.
    pub dir: Option<PathBuf>,
}

impl Skill {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }
    pub fn bundled(&self) -> bool {
        self.dir.is_none()
    }
    pub fn tool(&self, name: &str) -> Option<&ToolManifest> {
        self.manifest.tools.iter().find(|t| t.name == name)
    }
}

/// Everything found on disk plus the built-ins.
#[derive(Debug, Default)]
pub struct Catalog {
    pub skills: Vec<Arc<Skill>>,
    /// Skills that failed to load: (folder name, error).
    pub invalid: Vec<(String, String)>,
}

impl Catalog {
    pub fn get(&self, id: &str) -> Option<&Arc<Skill>> {
        self.skills.iter().find(|s| s.id() == id)
    }
}

pub type Fingerprint = Vec<(String, Option<SystemTime>, Option<SystemTime>)>;

fn fingerprint(dir: &Path) -> Fingerprint {
    let mtime = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut fp: Fingerprint = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| {
                    let p = e.path();
                    (e.file_name().to_string_lossy().into_owned(), mtime(p.join("skill.toml")), mtime(p.join("SKILL.md")))
                })
                .collect()
        })
        .unwrap_or_default();
    fp.sort();
    fp
}

/// Load built-ins and `skills_dir/*/skill.toml`. A user folder with the same
/// id as a built-in replaces it.
pub fn load_catalog(skills_dir: &Path) -> Catalog {
    let mut cat = Catalog::default();
    for (id, toml, guide) in BUNDLED {
        match parse_manifest(toml) {
            Ok(m) => cat.skills.push(Arc::new(Skill { manifest: m, guide: guide.to_string(), dir: None })),
            Err(e) => cat.invalid.push((id.to_string(), format!("built-in manifest: {e}"))),
        }
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(skills_dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.join("skill.toml").is_file()).collect())
        .unwrap_or_default();
    dirs.sort();
    for dir in dirs {
        let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let loaded = std::fs::read_to_string(dir.join("skill.toml"))
            .map_err(|e| e.to_string())
            .and_then(|t| parse_manifest(&t))
            .and_then(|m| if m.id == folder { Ok(m) } else { Err(format!("id '{}' must match the folder name '{folder}'", m.id)) });
        match loaded {
            Ok(m) => {
                let guide = std::fs::read_to_string(dir.join("SKILL.md")).unwrap_or_default();
                cat.skills.retain(|s| s.id() != m.id);
                cat.skills.push(Arc::new(Skill { manifest: m, guide, dir: Some(dir) }));
            }
            Err(e) => cat.invalid.push((folder, e)),
        }
    }
    // Tool names must be unique across skills: drop later duplicates.
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    let mut keep = Vec::new();
    for s in std::mem::take(&mut cat.skills) {
        match s.manifest.tools.iter().find_map(|t| owner.get(&t.name).map(|o| (t.name.clone(), o.clone()))) {
            Some((tool, other)) => cat.invalid.push((s.id().to_string(), format!("tool '{tool}' is already provided by '{other}'"))),
            None => {
                for t in &s.manifest.tools {
                    owner.insert(t.name.clone(), s.id().to_string());
                }
                keep.push(s);
            }
        }
    }
    cat.skills = keep;
    cat
}

/// Current catalog, reloaded when anything in the skills folder changes.
pub fn catalog(app: &App) -> Arc<Catalog> {
    let dir = app.paths.skills();
    let fp = fingerprint(&dir);
    let mut cache = app.skill_cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((old, cat)) = cache.as_ref()
        && *old == fp {
            return cat.clone();
        }
    let cat = Arc::new(load_catalog(&dir));
    for (folder, err) in &cat.invalid {
        crate::warn!("skill {folder}: {err}");
    }
    *cache = Some((fp, cat.clone()));
    cat
}

// ------------------------------------------------------------------ state

pub fn enabled(cfg: &Config, s: &Skill) -> bool {
    cfg.skills.get(s.id()).and_then(|c| c.enabled).unwrap_or(s.bundled())
}

pub fn group_enabled(cfg: &Config, s: &Skill, group: Option<&str>) -> bool {
    match (group, cfg.skills.get(s.id()).and_then(|c| c.groups.as_ref())) {
        (None, _) | (_, None) => true,
        (Some(g), Some(list)) => list.iter().any(|x| x == g),
    }
}

/// A setting: config, then secrets (secret settings), then the manifest default.
pub fn setting(app: &App, cfg: &Config, s: &Skill, key: &str) -> Option<String> {
    let spec = s.manifest.settings.get(key);
    let v = if spec.is_some_and(|x| x.secret) {
        app.secrets().get(&secret_key(s.id(), key)).map(str::to_string)
    } else {
        cfg.skills.get(s.id()).and_then(|c| c.settings.get(key)).cloned()
    };
    v.filter(|v| !v.is_empty()).or_else(|| spec.and_then(|x| x.default.clone()))
}

pub fn secret_key(skill: &str, key: &str) -> String {
    format!("skill.{skill}.{key}")
}

fn has_bin(name: &str) -> bool {
    if name.contains('/') {
        return Path::new(name).is_file();
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// ("ready" | "disabled" | "needs_setup" | "missing_dependency", detail)
pub fn status(app: &App, cfg: &Config, s: &Skill) -> (&'static str, String) {
    if !enabled(cfg, s) {
        return ("disabled", String::new());
    }
    if let Some(b) = s.manifest.requires.bins.iter().find(|b| !has_bin(b)) {
        return ("missing_dependency", format!("'{b}' is not installed"));
    }
    if let Some(k) = s.manifest.requires.settings.iter().find(|k| setting(app, cfg, s, k).is_none()) {
        let label = s.manifest.settings.get(k.as_str()).map(|x| x.label.as_str()).filter(|l| !l.is_empty()).unwrap_or(k.as_str());
        return ("needs_setup", format!("set {label}"));
    }
    if s.manifest.requires.auth && !auth::connected(app, s) {
        let what = s.manifest.auth.as_ref().map(|a| a.label.as_str()).filter(|l| !l.is_empty()).unwrap_or("account");
        return ("needs_setup", format!("connect {what}"));
    }
    ("ready", String::new())
}

// ------------------------------------------------------------------ registry

/// One tool as offered to models.
#[derive(Clone, Debug)]
pub struct RegisteredTool {
    pub skill: Arc<Skill>,
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub permission: Permission,
    pub risk: String,
    /// Usable now (skill ready, group on, builtin configured).
    pub available: bool,
    pub reason: String,
}

/// The Tool Registry: every tool of every loaded skill with its effective
/// permission and availability.
pub struct Registry {
    pub tools: Vec<RegisteredTool>,
    pub catalog: Arc<Catalog>,
}

/// Effective permission: per-tool override, then skill (or alias), then the manifest default.
pub fn permission(cfg: &Config, s: &Skill, t: &ToolManifest) -> Permission {
    let p = &cfg.tools.permissions;
    p.get(&t.name)
        .or_else(|| p.get(s.id()))
        .or_else(|| s.manifest.aliases.iter().find_map(|a| p.get(a)))
        .copied()
        .unwrap_or_else(|| t.default_permission())
}

pub fn registry(app: &App) -> Registry {
    let cfg = app.config();
    let cat = catalog(app);
    let mut tools = Vec::new();
    for s in &cat.skills {
        let (st, detail) = status(app, &cfg, s);
        for t in &s.manifest.tools {
            let mut reason = match st {
                "ready" => String::new(),
                "disabled" => "skill is disabled".into(),
                _ => detail.clone(),
            };
            if reason.is_empty() && !group_enabled(&cfg, s, t.group.as_deref()) {
                reason = format!("{} is turned off", t.group.as_deref().unwrap_or(""));
            }
            if reason.is_empty() && s.manifest.builtin.as_deref() == Some("web") && !crate::tools::web::available(&cfg, app, &t.name) {
                reason = "no search provider configured".into();
            }
            tools.push(RegisteredTool {
                skill: s.clone(),
                name: t.name.clone(),
                description: t.description.clone(),
                schema: t.schema().unwrap_or_else(|_| json!({ "type": "object" })),
                permission: permission(&cfg, s, t),
                risk: t.risk().to_string(),
                available: reason.is_empty(),
                reason,
            });
        }
    }
    Registry { tools, catalog: cat }
}

impl Registry {
    pub fn find(&self, name: &str) -> Option<&RegisteredTool> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// Tools a model may call right now.
    pub fn usable(&self) -> impl Iterator<Item = &RegisteredTool> {
        self.tools.iter().filter(|t| t.available && t.permission != Permission::Disabled)
    }

    /// Ready skills that have at least one usable tool.
    pub fn usable_skills(&self) -> Vec<Arc<Skill>> {
        let mut out: Vec<Arc<Skill>> = Vec::new();
        for t in self.usable() {
            if !out.iter().any(|s| s.id() == t.skill.id()) {
                out.push(t.skill.clone());
            }
        }
        out
    }

    /// Specs for model APIs, including `skill_guide`.
    pub fn specs(&self) -> Vec<ToolSpec> {
        let mut v: Vec<ToolSpec> =
            self.usable().map(|t| ToolSpec { name: t.name.clone(), description: t.description.clone(), parameters: t.schema.clone() }).collect();
        let skills = self.usable_skills();
        if !skills.is_empty() {
            let ids: Vec<&str> = skills.iter().map(|s| s.id()).collect();
            v.push(ToolSpec {
                name: GUIDE_TOOL.into(),
                description: "Read a skill's instructions (SKILL.md) before using its tools for the first time.".into(),
                parameters: json!({ "type": "object", "properties": { "skill": { "type": "string", "enum": ids } }, "required": ["skill"] }),
            });
        }
        v
    }

    /// Skill index for the system prompt (one line per usable skill).
    pub fn index(&self) -> String {
        let skills = self.usable_skills();
        if skills.is_empty() {
            return String::new();
        }
        let mut s = String::from("Skills (call skill_guide for instructions before first use):\n");
        for sk in skills {
            s.push_str(&format!("- {}: {}\n", sk.id(), sk.manifest.description));
        }
        s
    }
}

/// JSON for the settings UI and `alice skill`.
pub fn list_json(app: &App) -> Value {
    let cfg = app.config();
    let reg = registry(app);
    let skills: Vec<Value> = reg
        .catalog
        .skills
        .iter()
        .map(|s| {
            let (st, detail) = status(app, &cfg, s);
            let m = &s.manifest;
            let settings: Vec<Value> = m
                .settings
                .iter()
                .map(|(k, spec)| {
                    let v = setting(app, &cfg, s, k);
                    json!({
                        "key": k, "label": spec.label, "secret": spec.secret, "advanced": spec.advanced,
                        "required": m.requires.settings.contains(k),
                        // Secret values are never returned, only whether they are set.
                        "value": if spec.secret { Value::Null } else { json!(v.clone().unwrap_or_default()) },
                        "set": v.is_some(),
                    })
                })
                .collect();
            let groups: Vec<Value> = m
                .groups
                .iter()
                .map(|g| json!({ "id": g.id, "label": g.label, "enabled": group_enabled(&cfg, s, Some(&g.id)) }))
                .collect();
            let tools: Vec<Value> = reg
                .tools
                .iter()
                .filter(|t| t.skill.id() == s.id())
                .map(|t| {
                    let tm = s.tool(&t.name);
                    json!({
                        "name": t.name, "description": t.description, "group": tm.and_then(|x| x.group.clone()),
                        "permission": t.permission.as_str(), "default": tm.map(|x| x.default_permission().as_str()),
                        "risk": t.risk, "handler": tm.map(|x| x.handler.clone()),
                        "available": t.available, "reason": t.reason,
                    })
                })
                .collect();
            json!({
                "id": m.id, "name": m.name, "version": m.version, "description": m.description,
                "source": if s.bundled() { "builtin".to_string() } else { s.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default() },
                "enabled": enabled(&cfg, s), "status": st, "detail": detail,
                "permissions": { "network": m.permissions.network, "files": m.permissions.files, "description": m.permissions.description },
                "requires": { "bins": m.requires.bins, "settings": m.requires.settings, "auth": m.requires.auth },
                "auth": m.auth.as_ref().map(|a| json!({ "type": a.kind, "label": a.label, "connected": auth::connected(app, s) })),
                "settings": settings, "groups": groups, "tools": tools,
                "has_guide": !s.guide.trim().is_empty(),
            })
        })
        .collect();
    let invalid: Vec<Value> = reg.catalog.invalid.iter().map(|(f, e)| json!({ "folder": f, "error": e })).collect();
    json!({ "skills": skills, "invalid": invalid, "dir": app.paths.skills().display().to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_manifests_parse() {
        let cat = load_catalog(Path::new("/nonexistent"));
        assert!(cat.invalid.is_empty(), "{:?}", cat.invalid);
        let ids: Vec<&str> = cat.skills.iter().map(|s| s.id()).collect();
        assert_eq!(ids, ["files", "web", "google-workspace"]);
        let g = cat.get("google-workspace").unwrap();
        assert_eq!(g.manifest.tools.len(), 13);
        assert!(g.tool("calendar_create").unwrap().schema().unwrap()["required"].as_array().unwrap().len() == 3);
        assert!(!g.guide.is_empty());
    }

    #[test]
    fn example_skill_parses() {
        let m = parse_manifest(include_str!("../../skills/examples/github/skill.toml")).unwrap();
        assert_eq!(m.tools.len(), 3);
        assert_eq!(m.auth.unwrap().kind, "token");
    }

    #[test]
    fn rejects_bad_manifests_and_duplicates() {
        assert!(parse_manifest("id = \"Bad\"\nname = \"x\"\ndescription = \"x\"").is_err());
        assert!(parse_manifest("id = \"x\"\nname = \"x\"\ndescription = \"x\"\n[[tools]]\nname = \"t\"\ndescription = \"d\"\nhandler = \"http\"").is_err());
        assert!(parse_manifest("id = \"x\"\nname = \"x\"\ndescription = \"x\"\nunknown = 1").is_err());

        let dir = tempfile::tempdir().unwrap();
        let mk = |folder: &str, body: &str| {
            std::fs::create_dir_all(dir.path().join(folder)).unwrap();
            std::fs::write(dir.path().join(folder).join("skill.toml"), body).unwrap();
        };
        // Steals a built-in tool name.
        mk("thief", "id = \"thief\"\nname = \"T\"\ndescription = \"d\"\n[[tools]]\nname = \"fs_read\"\ndescription = \"d\"\nhandler = \"command\"\ncommand = [\"true\"]");
        // Folder/id mismatch.
        mk("other", "id = \"nope\"\nname = \"T\"\ndescription = \"d\"");
        mk("good", "id = \"good\"\nname = \"G\"\ndescription = \"d\"\n[[tools]]\nname = \"good_echo\"\ndescription = \"d\"\nhandler = \"command\"\ncommand = [\"cat\"]");
        let cat = load_catalog(dir.path());
        assert!(cat.get("good").is_some());
        assert!(cat.get("thief").is_none() && cat.get("nope").is_none());
        assert_eq!(cat.invalid.len(), 2, "{:?}", cat.invalid);
    }
}
