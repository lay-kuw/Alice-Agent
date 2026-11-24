//! Skill authentication: pasted tokens and OAuth 2.0 (authorization code +
//! PKCE + loopback redirect).
//!
//! Long-lived credentials (token / refresh token / client secret) live only in
//! secrets.toml (0600) under `skill.<id>.*`. Access tokens are kept in memory.
//! None of them are ever given to a model, the MCP relay, logs or API responses.

use super::{Skill, secret_key, setting};
use crate::app::App;
use crate::util::{b64url_encode, new_token, urlencode};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct AuthState {
    /// state -> pending login
    pending: Mutex<HashMap<String, Pending>>,
    /// skill id -> (access token, expiry)
    access: Mutex<HashMap<String, (String, Instant)>>,
}

struct Pending {
    skill: String,
    verifier: String,
    redirect_uri: String,
    created: Instant,
}

const LOGIN_TTL: Duration = Duration::from_secs(900);

fn kind(s: &Skill) -> &str {
    s.manifest.auth.as_ref().map(|a| a.kind.as_str()).unwrap_or("none")
}

pub fn connected(app: &App, s: &Skill) -> bool {
    let secrets = app.secrets();
    match kind(s) {
        "token" => secrets.get(&secret_key(s.id(), "token")).is_some(),
        "oauth2" => secrets.get(&secret_key(s.id(), "refresh_token")).is_some(),
        _ => true,
    }
}

/// `{{settings.key}}` in manifest URLs (endpoint overrides).
fn expand(app: &App, s: &Skill, template: &str) -> String {
    let cfg = app.config();
    let mut out = template.to_string();
    for key in s.manifest.settings.keys() {
        let pat = format!("{{{{settings.{key}}}}}");
        if out.contains(&pat) {
            out = out.replace(&pat, &setting(app, &cfg, s, key).unwrap_or_default());
        }
    }
    out
}

fn scopes(app: &App, s: &Skill) -> Vec<String> {
    let cfg = app.config();
    let mut v: Vec<String> = Vec::new();
    for g in &s.manifest.groups {
        if super::group_enabled(&cfg, s, Some(&g.id)) {
            for sc in &g.scopes {
                if !v.contains(sc) {
                    v.push(sc.clone());
                }
            }
        }
    }
    v
}

/// Save a pasted token (`auth.type = "token"`).
pub fn set_token(app: &App, s: &Skill, token: &str) -> Result<(), String> {
    if kind(s) != "token" {
        return Err(format!("{} does not use a pasted token", s.id()));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err("empty token".into());
    }
    app.update_secrets(|x| x.set(&secret_key(s.id(), "token"), token))
}

/// Begin an OAuth login; returns the URL to open in a browser.
pub fn login_url(app: &App, s: &Skill, redirect_uri: &str) -> Result<String, String> {
    let a = s.manifest.auth.as_ref().filter(|a| a.kind == "oauth2").ok_or_else(|| format!("{} does not use OAuth", s.id()))?;
    let cfg = app.config();
    let client_id = setting(app, &cfg, s, "client_id").ok_or("OAuth client ID is not set")?;
    let verifier = new_token() + &new_token();
    let state = new_token();
    let mut u = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&state={}",
        expand(app, s, &a.authorize_url),
        urlencode(&client_id),
        urlencode(redirect_uri),
        state
    );
    let sc = scopes(app, s);
    if !sc.is_empty() {
        u.push_str(&format!("&scope={}", urlencode(&sc.join(" "))));
    }
    if a.pkce {
        u.push_str(&format!("&code_challenge={}&code_challenge_method=S256", b64url_encode(&Sha256::digest(verifier.as_bytes()))));
    }
    for (k, v) in &a.params {
        u.push_str(&format!("&{}={}", urlencode(k), urlencode(v)));
    }
    let mut p = app.auth.pending.lock().unwrap_or_else(|e| e.into_inner());
    p.retain(|_, x| x.created.elapsed() < LOGIN_TTL);
    p.insert(state, Pending { skill: s.id().to_string(), verifier, redirect_uri: redirect_uri.to_string(), created: Instant::now() });
    Ok(u)
}

/// Complete a login with `code` and `state` from the redirect. Returns the skill id.
pub async fn finish_login(app: &App, code: &str, state: &str) -> Result<String, String> {
    let pending = {
        let mut p = app.auth.pending.lock().unwrap_or_else(|e| e.into_inner());
        let key = p.keys().find(|k| crate::util::ct_eq(k.as_bytes(), state.as_bytes())).cloned();
        key.and_then(|k| p.remove(&k)).ok_or("unknown or already used login; start again")?
    };
    if pending.created.elapsed() > LOGIN_TTL {
        return Err("login expired; start again".into());
    }
    let cat = super::catalog(app);
    let s = cat.get(&pending.skill).ok_or("skill no longer exists")?.clone();
    let a = s.manifest.auth.clone().ok_or("skill has no auth")?;
    let cfg = app.config();
    let client_id = setting(app, &cfg, &s, "client_id").unwrap_or_default();
    let client_secret = setting(app, &cfg, &s, "client_secret").unwrap_or_default();
    let mut form = vec![
        ("client_id", client_id.as_str()),
        ("code", code),
        ("grant_type", "authorization_code"),
        ("redirect_uri", pending.redirect_uri.as_str()),
    ];
    if a.pkce {
        form.push(("code_verifier", pending.verifier.as_str()));
    }
    if !client_secret.is_empty() {
        form.push(("client_secret", client_secret.as_str()));
    }
    let v = token_request(app, &expand(app, &s, &a.token_url), &form).await?;
    let refresh = v["refresh_token"].as_str().ok_or_else(|| {
        format!("token exchange failed: {}", v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("no refresh token returned"))
    })?;
    app.update_secrets(|x| x.set(&secret_key(s.id(), "refresh_token"), refresh))?;
    remember_access(app, s.id(), &v);
    crate::info!("skill {} connected", s.id());
    Ok(s.id().to_string())
}

async fn token_request(app: &App, url: &str, form: &[(&str, &str)]) -> Result<Value, String> {
    let r = app.http.post(url).form(form).timeout(Duration::from_secs(30)).send().await.map_err(|e| e.without_url().to_string())?;
    r.json().await.map_err(|e| e.without_url().to_string())
}

fn remember_access(app: &App, skill: &str, v: &Value) {
    if let Some(at) = v["access_token"].as_str() {
        let exp = Instant::now() + Duration::from_secs(v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60));
        app.auth.access.lock().unwrap_or_else(|e| e.into_inner()).insert(skill.to_string(), (at.to_string(), exp));
    }
}

/// Forget credentials (revoking OAuth tokens at the provider first, best effort).
pub async fn logout(app: &App, s: &Skill) -> Result<(), String> {
    if kind(s) == "oauth2"
        && let Some(t) = app.secrets().get(&secret_key(s.id(), "refresh_token")).map(str::to_string)
        && let Some(a) = &s.manifest.auth
        && !a.revoke_url.is_empty() {
            let _ = app.http.post(expand(app, s, &a.revoke_url)).form(&[("token", t.as_str())]).timeout(Duration::from_secs(15)).send().await;
        }
    app.auth.access.lock().unwrap_or_else(|e| e.into_inner()).remove(s.id());
    app.update_secrets(|x| {
        x.set(&secret_key(s.id(), "refresh_token"), "");
        x.set(&secret_key(s.id(), "token"), "");
    })
}

/// Credential to send to the skill's API, refreshing OAuth access tokens as needed.
pub async fn access_token(app: &App, s: &Skill) -> Result<String, String> {
    match kind(s) {
        "token" => app.secrets().get(&secret_key(s.id(), "token")).map(str::to_string).ok_or_else(|| format!("{} is not connected (alice skill login {})", s.manifest.name, s.id())),
        "oauth2" => {
            if let Some((t, exp)) = app.auth.access.lock().unwrap_or_else(|e| e.into_inner()).get(s.id()).cloned()
                && Instant::now() < exp {
                    return Ok(t);
                }
            let refresh = app
                .secrets()
                .get(&secret_key(s.id(), "refresh_token"))
                .map(str::to_string)
                .ok_or_else(|| format!("{} is not connected (alice skill login {})", s.manifest.name, s.id()))?;
            let a = s.manifest.auth.as_ref().ok_or("no auth")?;
            let cfg = app.config();
            let client_id = setting(app, &cfg, s, "client_id").unwrap_or_default();
            let client_secret = setting(app, &cfg, s, "client_secret").unwrap_or_default();
            let mut form = vec![("client_id", client_id.as_str()), ("refresh_token", refresh.as_str()), ("grant_type", "refresh_token")];
            if !client_secret.is_empty() {
                form.push(("client_secret", client_secret.as_str()));
            }
            let v = token_request(app, &expand(app, s, &a.token_url), &form).await?;
            let Some(at) = v["access_token"].as_str() else {
                let err = v["error"].as_str().unwrap_or("token refresh failed");
                return Err(if err == "invalid_grant" {
                    format!("{} authorization expired or was revoked; reconnect with `alice skill login {}`", s.manifest.name, s.id())
                } else {
                    format!("{} token refresh failed: {err}", s.manifest.name)
                });
            };
            let at = at.to_string();
            remember_access(app, s.id(), &v);
            Ok(at)
        }
        _ => Err("this skill has no credentials".into()),
    }
}

/// Every credential string of a skill, for scrubbing outputs and logs.
pub fn secrets_of(app: &App, s: &Skill) -> Vec<String> {
    let secrets = app.secrets();
    let mut v: Vec<String> = ["token", "refresh_token", "client_secret"]
        .iter()
        .filter_map(|k| secrets.get(&secret_key(s.id(), k)).map(str::to_string))
        .collect();
    if let Some((t, _)) = app.auth.access.lock().unwrap_or_else(|e| e.into_inner()).get(s.id()) {
        v.push(t.clone());
    }
    v.retain(|x| x.len() >= 8);
    v
}

/// Parse `code` and `state` from a pasted redirect URL (headless login).
pub fn parse_redirect(u: &str) -> Option<(String, String)> {
    let q = u.split_once('?')?.1;
    let mut code = None;
    let mut state = None;
    for kv in q.split('&') {
        let (k, v) = kv.split_once('=')?;
        let v = percent_decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            _ => {}
        }
    }
    Some((code?, state?))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len()
            && let Ok(n) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(n);
                i += 3;
                continue;
            }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
