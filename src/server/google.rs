//! `/api/google/*`: Calendar, Gmail and Tasks for the apps (iOS).
//!
//! These are the user's own actions from an authenticated client, so they do
//! not ask for approval. They still follow the skill: it must be enabled and
//! connected, and each service only works while its group is turned on.
//! Tokens never leave the server.

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
use std::sync::Arc;

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
        .route("/google/tasks", get(tasks))
        .route("/google/tasks/{list}", post(create_task))
        .route("/google/tasks/{list}/{id}", patch(update_task).delete(delete_task))
}

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
        email = g::gmail_address(&G::new(&app, &s)).await.map(Value::from).unwrap_or(Value::Null);
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
    let list = g::calendar_events(&G::new(&app, &s), &r.from, &r.to, r.q.as_deref(), 250).await.map_err(upstream("calendar list"))?;
    Ok(Json(json!({ "events": list })))
}

async fn create_event(State(app): State<AppRef>, Json(b): Json<Value>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "calendar")?;
    let e = g::calendar_create(&G::new(&app, &s), &b).await.map_err(upstream("calendar create"))?;
    log("calendar event created");
    Ok(Json(json!({ "event": e })))
}

async fn delete_event(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "calendar")?;
    g::calendar_delete(&G::new(&app, &s), &id).await.map_err(upstream("calendar delete"))?;
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
    let (list, next) = g::gmail_list(&G::new(&app, &s), &q, m.max.unwrap_or(25).clamp(1, 50), m.page.as_deref()).await.map_err(upstream("mail list"))?;
    Ok(Json(json!({ "messages": list, "next_page": next })))
}

async fn mail_one(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "gmail")?;
    let m = g::gmail_get(&G::new(&app, &s), &id).await.map_err(upstream("mail read"))?;
    Ok(Json(json!({ "message": m })))
}

#[derive(Deserialize)]
struct Send {
    to: String,
    cc: Option<String>,
    subject: String,
    body: String,
    reply_to_id: Option<String>,
}

async fn send_mail(State(app): State<AppRef>, Json(b): Json<Send>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "gmail")?;
    if b.to.trim().is_empty() {
        return Err(super::bad("to is required"));
    }
    let id = g::gmail_send(&G::new(&app, &s), b.to.trim(), b.cc.as_deref(), &b.subject, &b.body, b.reply_to_id.as_deref())
        .await
        .map_err(upstream("mail send"))?;
    log("mail sent");
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
struct TasksQuery {
    completed: Option<bool>,
}

async fn tasks(State(app): State<AppRef>, Query(q): Query<TasksQuery>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "tasks")?;
    let lists = g::tasks_all(&G::new(&app, &s), q.completed.unwrap_or(true)).await.map_err(upstream("tasks list"))?;
    Ok(Json(json!({ "lists": lists })))
}

async fn create_task(State(app): State<AppRef>, Path(list): Path<String>, Json(b): Json<Value>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "tasks")?;
    let t = g::task_create(&G::new(&app, &s), Some(&list), &b).await.map_err(upstream("task create"))?;
    log("task created");
    Ok(Json(json!({ "task": t })))
}

async fn update_task(State(app): State<AppRef>, Path((list, id)): Path<(String, String)>, Json(b): Json<Value>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "tasks")?;
    let t = g::task_update(&G::new(&app, &s), &list, &id, &b).await.map_err(upstream("task update"))?;
    log("task updated");
    Ok(Json(json!({ "task": t })))
}

async fn delete_task(State(app): State<AppRef>, Path((list, id)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let s = ready(&app, "tasks")?;
    g::task_delete(&G::new(&app, &s), &list, &id).await.map_err(upstream("task delete"))?;
    log("task deleted");
    Ok(Json(json!({ "ok": true })))
}
