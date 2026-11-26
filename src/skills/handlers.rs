//! Tool handlers: built-in modules, declarative HTTP requests and local
//! commands. Handlers never decide permissions; [`super::gate`] does.

use super::{Skill, ToolManifest, auth, setting};
use crate::app::{App, AppRef};
use crate::tools::untrusted;
use crate::util::urlencode;
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_OUTPUT: usize = 256 * 1024;

pub async fn run(app: &AppRef, s: &Skill, t: &ToolManifest, args: &Value) -> Result<String, String> {
    match t.handler.as_str() {
        "builtin" => builtin(app, s, &t.name, args).await,
        "http" => http(app, s, t, args).await,
        "command" => command(app, s, t, args).await,
        h => Err(format!("unknown handler {h}")),
    }
}

async fn builtin(app: &AppRef, s: &Skill, name: &str, args: &Value) -> Result<String, String> {
    let cfg = app.config();
    match s.manifest.builtin.as_deref() {
        Some("fs") => crate::tools::fs::execute(&cfg, &app.paths, name, args).await,
        Some("web") => crate::tools::web::execute(app, &cfg, name, args).await,
        Some("google") => crate::tools::google::execute(app, s, name, args).await,
        Some("recall") => crate::tools::recall::execute(app, name, args).await,
        Some("background") => {
            let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("");
            let t = crate::schedule::start_background(app, s("title"), s("instructions"))?;
            Ok(format!("Started in the background in a new chat \"{}\". The result will be posted there and the user notified.", t.title))
        }
        _ => Err("no builtin module".into()),
    }
}

/// Value for `{{name}}`: the argument, else a skill setting of the same name.
fn lookup(app: &App, s: &Skill, args: &Value, key: &str) -> Option<String> {
    let cfg = app.config();
    if let Some(k) = key.strip_prefix("settings.") {
        return setting(app, &cfg, s, k).filter(|_| !s.manifest.settings.get(k).is_some_and(|x| x.secret));
    }
    match args.get(key) {
        Some(Value::String(v)) => Some(v.clone()),
        Some(Value::Null) | None => setting(app, &cfg, s, key).filter(|_| !s.manifest.settings.get(key).is_some_and(|x| x.secret)),
        Some(v) => Some(v.to_string()),
    }
}

/// Fill `{{…}}` placeholders. Values from arguments are URL-encoded so they
/// cannot change the path structure or host; settings (user-controlled) are not.
pub fn fill_url(template: &str, get: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let j = after.find("}}").ok_or("unclosed {{ in url")?;
        let key = after[..j].trim();
        let v = get(key).ok_or_else(|| format!("missing argument: {key}"))?;
        if key.starts_with("settings.") {
            out.push_str(v.trim_end_matches('/'));
        } else {
            out.push_str(&urlencode(&v));
        }
        rest = &after[j + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn host_allowed(url: &str, allowed: &[String]) -> Result<(), String> {
    let u = reqwest::Url::parse(url).map_err(|e| format!("bad url: {e}"))?;
    if !matches!(u.scheme(), "https" | "http") {
        return Err("only http(s) urls are allowed".into());
    }
    let host = u.host_str().unwrap_or("");
    if allowed.iter().any(|a| a == "*" || a.eq_ignore_ascii_case(host)) {
        Ok(())
    } else {
        Err(format!("host '{host}' is not declared in the skill's permissions.network"))
    }
}

async fn http(app: &App, s: &Skill, t: &ToolManifest, args: &Value) -> Result<String, String> {
    let get = |k: &str| lookup(app, s, args, k);
    let url = fill_url(t.url.as_deref().unwrap_or(""), &get)?;
    host_allowed(&url, &s.manifest.permissions.network)?;
    let method = reqwest::Method::from_bytes(t.method.as_deref().unwrap_or("GET").as_bytes()).map_err(|e| e.to_string())?;
    let mut rb = app.http.request(method, &url).timeout(Duration::from_secs(30));
    let query: Vec<(String, String)> = t.query.iter().filter_map(|k| args.get(k).map(|v| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())))).collect();
    if !query.is_empty() {
        rb = rb.query(&query);
    }
    if !t.body.is_empty() {
        let body: serde_json::Map<String, Value> = t.body.iter().filter_map(|k| args.get(k).map(|v| (k.clone(), v.clone()))).collect();
        rb = rb.json(&body);
    }
    for (k, v) in &t.headers {
        rb = rb.header(k.as_str(), fill_url(v, &get).unwrap_or_default());
    }
    if s.manifest.auth.as_ref().is_some_and(|a| a.kind != "none") {
        rb = rb.bearer_auth(auth::access_token(app, s).await?);
    }
    rb = rb.header("accept", "application/json");
    let r = rb.send().await.map_err(|e| e.without_url().to_string())?;
    let status = r.status();
    let bytes = r.bytes().await.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_OUTPUT)]).into_owned();
    let pretty = serde_json::from_str::<Value>(&text).ok().and_then(|v| serde_json::to_string_pretty(&v).ok()).unwrap_or(text);
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), crate::util::truncate(&pretty, 500)));
    }
    Ok(untrusted(s.id(), &pretty))
}

async fn command(app: &App, s: &Skill, t: &ToolManifest, args: &Value) -> Result<String, String> {
    let dir = s.dir.as_ref().ok_or("command tools need a skill folder")?;
    let cfg = app.config();
    let settings: serde_json::Map<String, Value> = s
        .manifest
        .settings
        .iter()
        .filter(|(_, spec)| !spec.secret)
        .filter_map(|(k, _)| setting(app, &cfg, s, k).map(|v| (k.clone(), json!(v))))
        .collect();
    let input = json!({ "tool": t.name, "arguments": args, "settings": settings }).to_string();
    let mut cmd = tokio::process::Command::new(&t.command[0]);
    cmd.args(&t.command[1..])
        .current_dir(dir)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for k in ["PATH", "HOME", "LANG", "TMPDIR"] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    if s.manifest.auth.as_ref().is_some_and(|a| a.kind != "none") {
        cmd.env("ALICE_SKILL_TOKEN", auth::access_token(app, s).await?);
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", t.command[0]))?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let run = async {
        let _ = stdin.write_all(input.as_bytes()).await;
        drop(stdin);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut so = (&mut stdout).take(MAX_OUTPUT as u64);
        let mut se = (&mut stderr).take(8192);
        let (a, b) = tokio::join!(so.read_to_end(&mut out), se.read_to_end(&mut err));
        a.map_err(|e| e.to_string())?;
        let _ = b;
        let status = child.wait().await.map_err(|e| e.to_string())?;
        Ok::<_, String>((status, out, err))
    };
    let secs = t.timeout_secs.unwrap_or(60).clamp(1, 600);
    let (status, out, err) = tokio::time::timeout(Duration::from_secs(secs), run).await.map_err(|_| format!("timed out after {secs}s"))??;
    let out = String::from_utf8_lossy(&out).into_owned();
    if !status.success() {
        let e = String::from_utf8_lossy(&err);
        return Err(format!("exit {}: {}", status.code().unwrap_or(-1), crate::util::truncate(if e.trim().is_empty() { &out } else { &e }, 500)));
    }
    // `{"ok": false, "error": "…"}` signals a tool-level error.
    if let Ok(v) = serde_json::from_str::<Value>(&out)
        && v["ok"] == false {
            return Err(v["error"].as_str().unwrap_or("tool failed").to_string());
        }
    Ok(untrusted(s.id(), &out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_templates_encode_arguments() {
        let get = |k: &str| match k {
            "repo" => Some("a/../b?x=1".to_string()),
            "settings.api_base" => Some("https://api.github.com/".to_string()),
            _ => None,
        };
        let u = fill_url("{{settings.api_base}}/repos/{{repo}}/issues", &get).unwrap();
        assert_eq!(u, "https://api.github.com/repos/a%2F..%2Fb%3Fx%3D1/issues");
        assert!(fill_url("{{missing}}", &get).is_err());
        assert!(host_allowed("https://api.github.com/x", &["api.github.com".into()]).is_ok());
        assert!(host_allowed("https://evil.example/x", &["api.github.com".into()]).is_err());
        assert!(host_allowed("file:///etc/passwd", &["*".into()]).is_err());
    }
}
