//! `/api/google/*`: Calendar, Gmail and Tasks for the apps (iOS).
//!
//! These are the user's own actions from an authenticated client, so they do
//! not ask for approval. They still follow the skill: it must be enabled and
//! connected, and each service only works while its group is turned on.
//! Tokens never leave the server.
//!
//! Reads are cached in memory (stale-while-revalidate) so the apps feel
//! instant: fresh entries are returned as-is, older ones are returned at once
//! while a background refresh runs. Writes drop the affected entries. Keys
//! include a hash of the Google connection, so logging out or switching
//! accounts never serves old data. Nothing is written to disk.

use super::{ApiError, ApiResult};
use crate::app::AppRef;
use crate::skills::{self, Skill};
use crate::tools::google::{self as g, G};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

const SKILL: &str = "google-workspace";
const GROUPS: [&str; 3] = ["calendar", "gmail", "tasks"];

pub fn routes() -> Router<AppRef> {
    Router::new()
        .route("/google", get(status))
        .route("/google/calendar/events", get(events).post(create_event))
        .route("/google/calendar/events/{id}", axum::routing::delete(delete_event))
        .route("/google/mail", get(mail))
        .route("/google/mail/send", post(send_mail))
        .route("/google/mail/{id}", get(mail_one))
        .route("/google/mail/{id}/read", post(mark_read))
        .route("/google/tasks", get(tasks))
        .route("/google/tasks/{list}", post(create_task))
        .route("/google/tasks/{list}/{id}", patch(update_task).delete(delete_task))
}

// ------------------------------------------------------------------ cache

const FRESH: Duration = Duration::from_secs(30);
const KEEP: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
struct Cache {
    entries: Mutex<HashMap<String, (Instant, Value)>>,
    refreshing: Mutex<HashSet<String>>,
}

static CACHE: LazyLock<Cache> = LazyLock::new(Cache::default);

/// Identifies the current Google connection and enabled groups.
fn epoch(app: &AppRef, s: &Skill) -> String {
    use sha2::Digest;
    let cfg = app.config();
    let rt = app.secrets().get(&skills::secret_key(s.id(), "refresh_token")).unwrap_or_default().to_string();
    let groups = GROUPS.iter().map(|g| skills::group_enabled(&cfg, s, Some(g)).to_string()).collect::<Vec<_>>().join(",");
    let h = sha2::Sha256::digest(format!("{}|{rt}|{groups}", app.paths.home.display()).as_bytes());
    h[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Cached read. `fresh`: how long an entry is served without refreshing.
async fn cached<F, Fut>(app: &AppRef, s: &Arc<Skill>, key: String, fresh: Duration, fetch: F) -> ApiResult<Value>
where
    F: Fn(AppRef, Arc<Skill>) -> Fut + Send + 'static,
    Fut: Future<Output = ApiResult<Value>> + Send + 'static,
{
    let key = format!("{}:{key}", epoch(app, s));
    let hit = CACHE.entries.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned();
    if let Some((at, v)) = hit {
        if at.elapsed() < fresh {
            return Ok(v);
        }
        if at.elapsed() < KEEP {
            // Serve now, refresh in the background (once per key).
            if CACHE.refreshing.lock().unwrap_or_else(|e| e.into_inner()).insert(key.clone()) {
                let (app, s, key) = (app.clone(), s.clone(), key.clone());
                tokio::spawn(async move {
                    if let Ok(v) = fetch(app, s).await {
                        store(&key, v);
                    }
                    CACHE.refreshing.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
                });
            }
            return Ok(v);
        }
    }
    let v = fetch(app.clone(), s.clone()).await?;
    store(&key, v.clone());
    Ok(v)
}

fn store(key: &str, v: Value) {
    let mut m = CACHE.entries.lock().unwrap_or_else(|e| e.into_inner());
    m.retain(|_, (at, _)| at.elapsed() < KEEP);
    m.insert(key.to_string(), (Instant::now(), v));
}

/// Drop cached reads whose key (after the epoch) starts with `prefix`.
fn invalidate(prefix: &str) {
    CACHE.entries.lock().unwrap_or_else(|e| e.into_inner()).retain(|k, _| !k.split_once(':').is_some_and(|(_, rest)| rest.starts_with(prefix)));
}

// ------------------------------------------------------------------ routes

fn skill(app: &AppRef) -> ApiResult<Arc<Skill>> {
    skills::catalog(app).get(SKILL).cloned().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "the google-workspace skill is not installed".into()))
}

/// The skill, if it is ready and `group` is turned on.
fn ready(app: &AppRef, group: &str) -> ApiResult<Arc<Skill>> {
    let s = skill(app)?;
    let cfg = app.config();
    let (state, detail) = skills::status(app, &cfg, &s);
    if state != "ready" {
        return Err(ApiError(StatusCode::CONFLICT, format!("Google is not ready: {}", if detail.is_empty() { state.to_string() } else { detail })));
    }
    if !skills::group_enabled(&cfg, &s, Some(group)) {
        return Err(ApiError(StatusCode::FORBIDDEN, format!("{group} is turned off in the Google Workspace skill")));
    }
    Ok(s)
}

/// Google errors as 502, with a hint when the token lacks a newly added scope.
fn upstream(action: &str) -> impl FnOnce(String) -> ApiError + '_ {
    move |e| {
        crate::warn!("google app: {action} failed: {e}");
        let hint = if e.contains("insufficient") || e.contains("HTTP 403") { " (reconnect Google to grant access)" } else { "" };
        ApiError(StatusCode::BAD_GATEWAY, format!("{e}{hint}"))
    }
}

fn log(action: &str) {
    crate::info!("google app: {action}");
}

async fn status(State(app): State<AppRef>) -> ApiResult<Json<Value>> {
    let s = skill(&app)?;
    let cfg = app.config();
    let (state, detail) = skills::status(&app, &cfg, &s);
    let groups: serde_json::Map<String, Value> = GROUPS.iter().map(|gr| (gr.to_string(), json!(skills::group_enabled(&cfg, &s, Some(gr))))).collect();
    let mut email = Value::Null;
    if state == "ready" && skills::group_enabled(&cfg, &s, Some("gmail")) {
        email = cached(&app, &s, "email".into(), KEEP, |app, s| async move { g::gmail_address(&G::new(&app, &s)).await.map(Value::from).map_err(upstream("profile")) })
            .await
            .unwrap_or(Value::Null);
    }
    Ok(Json(json!({ "status": state, "detail": detail, "groups": groups, "email": email })))
}

#[derive(Deserialize)]
struct Range {
    from: String,
    to: String,
    q: Option<String>,
}

async fn events(State(app): State<AppRef>, Query(r): Query<Range>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "calendar")?;
    let key = format!("cal:{}|{}|{}", r.from, r.to, r.q.clone().unwrap_or_default());
    let v = cached(&app, &s, key, FRESH, move |app, s| {
        let r = (r.from.clone(), r.to.clone(), r.q.clone());
        async move {
            let list = g::calendar_events(&G::new(&app, &s), &r.0, &r.1, r.2.as_deref(), 250).await.map_err(upstream("calendar list"))?;
            Ok(json!({ "events": list }))
        }
    })
    .await?;
    Ok(Json(v))
}

async fn create_event(State(app): State<AppRef>, Json(b): Json<Value>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "calendar")?;
    let e = g::calendar_create(&G::new(&app, &s), &b).await.map_err(upstream("calendar create"))?;
    invalidate("cal:");
    log("calendar event created");
    Ok(Json(json!({ "event": e })))
}

async fn delete_event(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "calendar")?;
    g::calendar_delete(&G::new(&app, &s), &id).await.map_err(upstream("calendar delete"))?;
    invalidate("cal:");
    log("calendar event deleted");
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct MailQuery {
    q: Option<String>,
    page: Option<String>,
    max: Option<u32>,
}

async fn mail(State(app): State<AppRef>, Query(m): Query<MailQuery>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "gmail")?;
    let q = m.q.filter(|q| !q.trim().is_empty()).unwrap_or_else(|| "in:inbox".into());
    let max = m.max.unwrap_or(25).clamp(1, 50);
    let key = format!("mail:{q}|{max}|{}", m.page.clone().unwrap_or_default());
    let v = cached(&app, &s, key, FRESH, move |app, s| {
        let (q, page) = (q.clone(), m.page.clone());
        async move {
            let (list, next) = g::gmail_list(&G::new(&app, &s), &q, max, page.as_deref()).await.map_err(upstream("mail list"))?;
            Ok(json!({ "messages": list, "next_page": next }))
        }
    })
    .await?;
    Ok(Json(v))
}

async fn mail_one(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "gmail")?;
    // A message's content never changes.
    let v = cached(&app, &s, format!("msg:{id}"), KEEP, move |app, s| {
        let id = id.clone();
        async move { Ok(json!({ "message": g::gmail_get(&G::new(&app, &s), &id).await.map_err(upstream("mail read"))? })) }
    })
    .await?;
    Ok(Json(v))
}

#[derive(Deserialize)]
struct MarkRead {
    read: Option<bool>,
}

async fn mark_read(State(app): State<AppRef>, Path(id): Path<String>, Json(b): Json<MarkRead>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "gmail")?;
    g::gmail_mark_read(&G::new(&app, &s), &id, b.read.unwrap_or(true)).await.map_err(upstream("mail mark read"))?;
    invalidate("mail:");
    invalidate(&format!("msg:{id}"));
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SendMail {
    to: String,
    cc: Option<String>,
    subject: String,
    body: String,
    reply_to_id: Option<String>,
}

async fn send_mail(State(app): State<AppRef>, Json(b): Json<SendMail>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "gmail")?;
    if b.to.trim().is_empty() {
        return Err(super::bad("to is required"));
    }
    let id = g::gmail_send(&G::new(&app, &s), b.to.trim(), b.cc.as_deref(), &b.subject, &b.body, b.reply_to_id.as_deref())
        .await
        .map_err(upstream("mail send"))?;
    invalidate("mail:");
    log("mail sent");
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
struct TasksQuery {
    completed: Option<bool>,
}

async fn tasks(State(app): State<AppRef>, Query(q): Query<TasksQuery>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "tasks")?;
    let completed = q.completed.unwrap_or(true);
    let v = cached(&app, &s, format!("tasks:{completed}"), FRESH, move |app, s| async move {
        Ok(json!({ "lists": g::tasks_all(&G::new(&app, &s), completed).await.map_err(upstream("tasks list"))? }))
    })
    .await?;
    Ok(Json(v))
}

async fn create_task(State(app): State<AppRef>, Path(list): Path<String>, Json(b): Json<Value>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "tasks")?;
    let t = g::task_create(&G::new(&app, &s), Some(&list), &b).await.map_err(upstream("task create"))?;
    invalidate("tasks:");
    log("task created");
    Ok(Json(json!({ "task": t })))
}

async fn update_task(State(app): State<AppRef>, Path((list, id)): Path<(String, String)>, Json(b): Json<Value>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "tasks")?;
    let t = g::task_update(&G::new(&app, &s), &list, &id, &b).await.map_err(upstream("task update"))?;
    invalidate("tasks:");
    log("task updated");
    Ok(Json(json!({ "task": t })))
}

async fn delete_task(State(app): State<AppRef>, Path((list, id)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "tasks")?;
    g::task_delete(&G::new(&app, &s), &list, &id).await.map_err(upstream("task delete"))?;
    invalidate("tasks:");
    log("task deleted");
    Ok(Json(json!({ "ok": true })))
}
