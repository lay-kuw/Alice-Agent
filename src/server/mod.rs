//! HTTP API (REST + SSE) and the embedded Web GUI.

use crate::agent::{self, TurnError, TurnInput};
use crate::app::{App, AppRef};
use crate::config::{Permission, ProviderConfig, ProviderKind};
use crate::util::{ct_eq, new_id, now_ms};
use crate::{memory, providers, storage, tools};
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;

const MAX_ATTACHMENT: usize = 25 * 1024 * 1024;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        crate::error!("db error: {e}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
    }
}

impl From<TurnError> for ApiError {
    fn from(e: TurnError) -> Self {
        let code = match e {
            TurnError::NotFound => StatusCode::NOT_FOUND,
            TurnError::Busy => StatusCode::CONFLICT,
            TurnError::BadRequest(_) => StatusCode::BAD_REQUEST,
            TurnError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(code, e.to_string())
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}
fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not found".into())
}

type ApiResult<T> = Result<T, ApiError>;

pub fn router(app: AppRef) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/logout", post(logout))
        .route("/events", get(events))
        .route("/threads", get(list_threads).post(create_thread).delete(delete_all_threads))
        .route("/threads/{id}", get(get_thread).patch(update_thread).delete(delete_thread))
        .route("/threads/{id}/messages", get(list_messages).post(post_message))
        .route("/threads/{id}/stop", post(stop))
        .route("/threads/{id}/attachments", post(upload_attachment))
        .route("/attachments/{id}", get(get_attachment))
        .route("/approvals", get(list_approvals))
        .route("/approvals/{id}", post(resolve_approval))
        .route("/models", get(list_models))
        .route("/models/default", put(set_default_model))
        .route("/models/test", post(test_model))
        .route("/providers", post(add_provider))
        .route("/providers/{id}", axum::routing::delete(remove_provider))
        .route("/secrets/{name}", put(set_secret).delete(delete_secret))
        .route("/memories", get(list_memories).delete(clear_memories))
        .route("/memories/status", get(memory_status))
        .route("/memories/{id}", get(get_memory))
        .route("/config", get(get_config).patch(patch_config))
        .route("/tools", get(list_tools).patch(patch_tools))
        .route_layer(middleware::from_fn_with_state(app.clone(), auth))
        .route("/health", get(health))
        .route("/login", post(login))
        .layer(DefaultBodyLimit::max(MAX_ATTACHMENT + 1024));

    Router::new()
        .nest("/api", api)
        .route("/", get(web_index))
        .route("/app.js", get(web_js))
        .route("/style.css", get(web_css))
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}

// ---------------------------------------------------------------- auth

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').map(str::trim).find_map(|kv| kv.strip_prefix("alice_session=")).map(str::to_string)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    raw.strip_prefix("Bearer ").map(|s| s.trim().to_string())
}

fn token_ok(app: &App, candidate: &str) -> bool {
    match app.secrets().get("server.token") {
        Some(t) => ct_eq(t.as_bytes(), candidate.as_bytes()),
        None => false,
    }
}

async fn auth(State(app): State<AppRef>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    if let Some(t) = bearer_token(headers) {
        if token_ok(&app, &t) {
            return next.run(req).await;
        }
    } else if let Some(t) = cookie_token(headers)
        && token_ok(&app, &t) {
            // Cookie auth: state-changing requests need the custom header,
            // which a cross-site form or simple request cannot set.
            let safe = matches!(*req.method(), Method::GET | Method::HEAD);
            if safe || headers.get("x-alice-csrf").is_some() {
                return next.run(req).await;
            }
            return ApiError(StatusCode::FORBIDDEN, "missing CSRF header".into()).into_response();
        }
    ApiError(StatusCode::UNAUTHORIZED, "unauthorized".into()).into_response()
}

async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: blob:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
    );
    res
}

#[derive(Deserialize)]
struct LoginBody {
    token: String,
}

async fn login(State(app): State<AppRef>, headers: HeaderMap, Json(b): Json<LoginBody>) -> ApiResult<Response> {
    if !token_ok(&app, b.token.trim()) {
        // Slow down guessing.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid token".into()));
    }
    let https = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https")
        || app.config().server.public_url.starts_with("https://");
    let cookie = format!(
        "alice_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=31536000{}",
        b.token.trim(),
        if https { "; Secure" } else { "" }
    );
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).map_err(|_| bad("bad token"))?);
    Ok(res)
}

async fn logout() -> Response {
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("alice_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    res
}

// ---------------------------------------------------------------- web

async fn web_index() -> Response {
    asset("text/html; charset=utf-8", include_str!("../../web/index.html"))
}
async fn web_js() -> Response {
    asset("text/javascript; charset=utf-8", include_str!("../../web/app.js"))
}
async fn web_css() -> Response {
    asset("text/css; charset=utf-8", include_str!("../../web/style.css"))
}
fn asset(ct: &'static str, body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

// ---------------------------------------------------------------- status

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "name": "alice", "version": env!("CARGO_PKG_VERSION") }))
}

/// RSS (KB) and CPU% of a process, via `ps` (portable across Linux/macOS).
pub fn proc_stats(pid: u32) -> (u64, f64) {
    let out = std::process::Command::new("ps").args(["-o", "rss=,%cpu=", "-p", &pid.to_string()]).output();
    let Ok(out) = out else { return (0, 0.0) };
    let s = String::from_utf8_lossy(&out.stdout);
    let mut it = s.split_whitespace();
    let rss = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let cpu = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    (rss, cpu)
}

async fn status(State(app): State<AppRef>) -> ApiResult<Json<Value>> {
    let counts = storage::counts(&app.db.lock())?;
    let pid = std::process::id();
    let (rss_kb, cpu) = tokio::task::spawn_blocking(move || proc_stats(pid)).await.unwrap_or((0, 0.0));
    let cfg = app.config();
    let generating: Vec<String> = app.active.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "pid": pid,
        "uptime_secs": app.started.elapsed().as_secs(),
        "rss_kb": rss_kb,
        "cpu_percent": cpu,
        "db_bytes": storage::db_size_bytes(&app.paths.db()),
        "counts": counts,
        "generating": generating,
        "default_model": cfg.agent.default_model,
        "bind": format!("{}:{}", cfg.server.bind, cfg.server.port),
        "home": app.paths.home.display().to_string(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })))
}

#[derive(Deserialize)]
struct EventsQuery {
    thread: Option<String>,
}

async fn events(State(app): State<AppRef>, Query(q): Query<EventsQuery>) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let mut rx = app.events.subscribe();
    let filter = q.thread;
    let stream = async_stream(move |tx| async move {
        let _ = tx.send(SseEvent::default().data(json!({ "type": "hello", "ts": now_ms() }).to_string())).await;
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Some(f) = &filter {
                        // Thread-filtered streams still receive global events (e.g. threads.cleared).
                        if ev.thread_id.as_deref().is_some_and(|t| t != f) {
                            continue;
                        }
                    }
                    if tx.send(SseEvent::default().data(ev.json.as_ref())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    // Client fell behind; tell it to resync from the REST API.
                    let msg = json!({ "type": "resync", "missed": n }).to_string();
                    if tx.send(SseEvent::default().data(msg)).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(20)))
}

/// Tiny channel-backed stream helper (avoids an extra dependency).
fn async_stream<F, Fut>(f: F) -> impl Stream<Item = Result<SseEvent, Infallible>>
where
    F: FnOnce(tokio::sync::mpsc::Sender<SseEvent>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel::<SseEvent>(64);
    tokio::spawn(f(tx));
    futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|e| (Ok(e), rx)) })
}

// ---------------------------------------------------------------- threads

fn resolve(app: &App, id: &str) -> ApiResult<String> {
    storage::resolve_thread_id(&app.db.lock(), id)?.ok_or_else(not_found)
}

async fn list_threads(State(app): State<AppRef>) -> ApiResult<Json<Value>> {
    let threads = storage::list_threads(&app.db.lock())?;
    let active = app.active.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<Value> = threads
        .into_iter()
        .map(|t| {
            let mut v = serde_json::to_value(&t).unwrap_or_default();
            v["generating"] = json!(active.contains_key(&t.id));
            v
        })
        .collect();
    Ok(Json(json!({ "threads": list })))
}

#[derive(Deserialize, Default)]
struct CreateThread {
    #[serde(default)]
    title: String,
    model: Option<String>,
}

async fn create_thread(State(app): State<AppRef>, body: Option<Json<CreateThread>>) -> ApiResult<(StatusCode, Json<Value>)> {
    let b = body.map(|b| b.0).unwrap_or_default();
    if let Some(m) = b.model.as_deref().filter(|m| !m.is_empty())
        && app.config().resolve_model(m).is_none() {
            return Err(bad(format!("unknown model: {m}")));
        }
    let t = storage::create_thread(&app.db.lock(), b.title.trim(), b.model.as_deref().filter(|m| !m.is_empty()))?;
    app.emit(json!({ "type": "thread.created", "thread_id": t.id, "thread": t }));
    Ok((StatusCode::CREATED, Json(json!({ "thread": t }))))
}

async fn get_thread(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let id = resolve(&app, &id)?;
    let t = storage::get_thread(&app.db.lock(), &id)?.ok_or_else(not_found)?;
    Ok(Json(json!({ "thread": t, "generating": app.is_generating(&id) })))
}

#[derive(Deserialize)]
struct UpdateThread {
    title: Option<String>,
    /// Some("") clears the thread model (use the default).
    model: Option<String>,
}

async fn update_thread(State(app): State<AppRef>, Path(id): Path<String>, Json(b): Json<UpdateThread>) -> ApiResult<Json<Value>> {
    let id = resolve(&app, &id)?;
    if let Some(m) = b.model.as_deref().filter(|m| !m.is_empty())
        && app.config().resolve_model(m).is_none() {
            return Err(bad(format!("unknown model: {m}")));
        }
    let title = b.title.as_deref().map(str::trim);
    let model = b.model.as_deref().map(|m| if m.is_empty() { None } else { Some(m) });
    let t = storage::update_thread(&app.db.lock(), &id, title, model)?.ok_or_else(not_found)?;
    app.emit(json!({ "type": "thread.updated", "thread_id": t.id, "thread": t }));
    Ok(Json(json!({ "thread": t })))
}

/// Delete one thread and everything derived from it.
pub fn delete_thread_now(app: &App, id: &str) -> Result<bool, rusqlite::Error> {
    app.cancel_generation(id);
    app.drop_approvals(Some(id));
    let existed = {
        let mut c = app.db.lock();
        let existed = memory::delete_thread(&mut c, id)?;
        if existed {
            storage::optimize_fts(&c)?;
            storage::checkpoint(&c)?;
        }
        existed
    };
    let dir = app.paths.attachments().join(id);
    if dir.starts_with(app.paths.attachments()) && id.chars().all(|c| c.is_ascii_alphanumeric()) {
        let _ = std::fs::remove_dir_all(dir);
    }
    if existed {
        app.emit(json!({ "type": "thread.deleted", "thread_id": id }));
    }
    Ok(existed)
}

pub fn delete_all_now(app: &App) -> Result<(), rusqlite::Error> {
    let ids: Vec<String> = app.active.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
    for id in &ids {
        app.cancel_generation(id);
    }
    app.drop_approvals(None);
    {
        let mut c = app.db.lock();
        memory::delete_all(&mut c)?;
    }
    if let Ok(rd) = std::fs::read_dir(app.paths.attachments()) {
        for e in rd.flatten() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    app.emit(json!({ "type": "threads.cleared" }));
    Ok(())
}

async fn delete_thread(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let id = resolve(&app, &id)?;
    let a = app.clone();
    let existed = tokio::task::spawn_blocking(move || delete_thread_now(&a, &id))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    if existed { Ok(StatusCode::NO_CONTENT) } else { Err(not_found()) }
}

#[derive(Deserialize)]
struct ConfirmBody {
    confirm: String,
}

async fn delete_all_threads(State(app): State<AppRef>, Json(b): Json<ConfirmBody>) -> ApiResult<StatusCode> {
    if b.confirm != "delete-all" {
        return Err(bad("send {\"confirm\":\"delete-all\"}"));
    }
    let a = app.clone();
    tokio::task::spawn_blocking(move || delete_all_now(&a))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_messages(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let id = resolve(&app, &id)?;
    let c = app.db.lock();
    let msgs = storage::list_messages(&c, &id)?;
    let mut st = c.prepare_cached("SELECT id, message_id, name, mime, size FROM attachments WHERE thread_id = ?1")?;
    let atts: Vec<Value> = st
        .query_map([&id], |r| {
            Ok(json!({ "id": r.get::<_, String>(0)?, "message_id": r.get::<_, Option<String>>(1)?,
                       "name": r.get::<_, String>(2)?, "mime": r.get::<_, String>(3)?, "size": r.get::<_, i64>(4)? }))
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(json!({ "thread_id": id, "messages": msgs, "attachments": atts, "generating": app.is_generating(&id) })))
}

#[derive(Deserialize)]
struct PostMessage {
    content: String,
    #[serde(default)]
    client: String,
    model: Option<String>,
    #[serde(default)]
    attachments: Vec<String>,
}

async fn post_message(State(app): State<AppRef>, Path(id): Path<String>, Json(b): Json<PostMessage>) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = resolve(&app, &id)?;
    let client = match b.client.as_str() {
        "cli" | "web" | "ios" | "telegram" | "discord" => b.client.clone(),
        _ => "api".into(),
    };
    let started = agent::start_turn(
        &app,
        TurnInput { thread_id: id, text: b.content, client, model: b.model, attachment_ids: b.attachments },
    )?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "user_message": started.user_message,
            "assistant_message": started.assistant_message,
            "model": started.model,
        })),
    ))
}

async fn stop(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let id = resolve(&app, &id)?;
    Ok(Json(json!({ "stopped": app.cancel_generation(&id) })))
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
    mime: Option<String>,
}

async fn upload_attachment(
    State(app): State<AppRef>,
    Path(id): Path<String>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = resolve(&app, &id)?;
    if body.is_empty() {
        return Err(bad("empty file"));
    }
    if body.len() > MAX_ATTACHMENT {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "file too large".into()));
    }
    let name: String = q.name.chars().filter(|c| !c.is_control() && *c != '/' && *c != '\\').take(200).collect();
    let mime = q.mime.filter(|m| m.len() < 100 && m.contains('/')).unwrap_or_else(|| guess_mime(&name).into());
    let aid = new_id();
    let dir = app.paths.attachments().join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    std::fs::write(dir.join(&aid), &body).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let r = app.db.lock().execute(
        "INSERT INTO attachments (id, thread_id, name, mime, size, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![aid, id, name, mime, body.len() as i64, now_ms()],
    );
    if let Err(e) = r {
        let _ = std::fs::remove_file(dir.join(&aid));
        return Err(e.into());
    }
    Ok((StatusCode::CREATED, Json(json!({ "id": aid, "name": name, "mime": mime, "size": body.len() }))))
}

pub fn guess_mime(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "md" | "txt" | "csv" | "log" | "rs" | "py" | "js" | "ts" | "swift" | "toml" | "yaml" | "yml" | "html" | "css" => "text/plain",
        _ => "application/octet-stream",
    }
}

async fn get_attachment(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Response> {
    let row: Option<(String, String, String)> = {
        let c = app.db.lock();
        rusqlite::OptionalExtension::optional(c.query_row(
            "SELECT thread_id, name, mime FROM attachments WHERE id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ))?
    };
    let (thread_id, name, mime) = row.ok_or_else(not_found)?;
    let bytes = std::fs::read(app.paths.attachments().join(&thread_id).join(&id)).map_err(|_| not_found())?;
    // Never render uploaded content inline as HTML.
    let ct = if mime.starts_with("image/") && mime != "image/svg+xml" { mime } else { "application/octet-stream".into() };
    let disp = format!("attachment; filename=\"{}\"", name.replace(['"', '\r', '\n'], "_"));
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, ct)
        .header(header::CONTENT_DISPOSITION, disp)
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(bytes))
        .unwrap())
}

// ---------------------------------------------------------------- approvals

async fn list_approvals(State(app): State<AppRef>) -> Json<Value> {
    let ap = app.approvals.lock().unwrap_or_else(|e| e.into_inner());
    let ids: Vec<Value> = ap.iter().map(|(k, p)| json!({ "approval_id": k, "thread_id": p.thread_id })).collect();
    Json(json!({ "approvals": ids }))
}

#[derive(Deserialize)]
struct ApprovalBody {
    approve: bool,
}

async fn resolve_approval(State(app): State<AppRef>, Path(id): Path<String>, Json(b): Json<ApprovalBody>) -> ApiResult<Json<Value>> {
    let p = app.approvals.lock().unwrap_or_else(|e| e.into_inner()).remove(&id).ok_or_else(not_found)?;
    let _ = p.reply.send(b.approve);
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- models / providers

async fn list_models(State(app): State<AppRef>) -> Json<Value> {
    let cfg = app.config();
    let secrets = app.secrets();
    let providers: Vec<Value> = cfg
        .providers
        .iter()
        .map(|p| {
            json!({
                "id": p.id, "kind": p.kind.as_str(), "base_url": p.base_url(),
                "models": p.models, "context_tokens": p.context_tokens,
                "has_key": secrets.provider_key(&p.id).is_some(),
                "capabilities": providers::capabilities(p.kind),
            })
        })
        .collect();
    Json(json!({ "default": cfg.agent.default_model, "models": cfg.all_models(), "providers": providers }))
}

#[derive(Deserialize)]
struct ModelBody {
    model: String,
}

async fn set_default_model(State(app): State<AppRef>, Json(b): Json<ModelBody>) -> ApiResult<Json<Value>> {
    let cfg = app
        .update_config(|c| {
            if c.resolve_model(&b.model).is_none() {
                return Err(format!("unknown model: {}", b.model));
            }
            c.agent.default_model = b.model.clone();
            Ok(())
        })
        .map_err(bad)?;
    app.emit(json!({ "type": "config.updated" }));
    Ok(Json(json!({ "default": cfg.agent.default_model })))
}

async fn test_model(State(app): State<AppRef>, Json(b): Json<ModelBody>) -> ApiResult<Json<Value>> {
    let cfg = app.config();
    let (p, name) = cfg.resolve_model(&b.model).ok_or_else(|| bad(format!("unknown model: {}", b.model)))?;
    let key = app.secrets().provider_key(&p.id).map(str::to_string);
    let req = providers::ChatRequest {
        model: name,
        system: String::new(),
        messages: vec![providers::ChatMessage::user("Reply with the single word: ok")],
        tools: vec![],
        max_tokens: 1000,
    };
    let t0 = std::time::Instant::now();
    let mut first: Option<u128> = None;
    let mut sink = |_: &str| {
        if first.is_none() {
            first = Some(t0.elapsed().as_millis());
        }
    };
    let r = providers::stream(&app.http, p, key.as_deref(), &req, &mut sink).await;
    let total = t0.elapsed().as_millis();
    Ok(Json(match r {
        Ok(c) => json!({ "ok": true, "reply": crate::util::truncate(&c.text, 200), "ttft_ms": first, "total_ms": total, "usage": c.usage }),
        Err(e) => json!({ "ok": false, "error": e.0, "total_ms": total }),
    }))
}

#[derive(Deserialize)]
struct AddProvider {
    id: String,
    kind: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    models: Vec<String>,
    #[serde(default)]
    context_tokens: u32,
    api_key: Option<String>,
}

fn valid_ident(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

async fn add_provider(State(app): State<AppRef>, Json(b): Json<AddProvider>) -> ApiResult<Json<Value>> {
    if !valid_ident(&b.id) {
        return Err(bad("provider id: letters, digits, '-' or '_'"));
    }
    let kind = ProviderKind::parse(&b.kind).ok_or_else(|| bad(format!("unknown provider kind: {}", b.kind)))?;
    if kind == ProviderKind::OpenaiCompatible && b.base_url.is_empty() {
        return Err(bad("base_url is required for openai-compatible"));
    }
    let models: Vec<String> = b.models.iter().map(|m| m.trim().to_string()).filter(|m| !m.is_empty() && !m.contains(char::is_whitespace)).collect();
    if models.is_empty() {
        return Err(bad("at least one model name is required"));
    }
    let cfg = app
        .update_config(|c| {
            let p = ProviderConfig { id: b.id.clone(), kind, base_url: b.base_url.trim().to_string(), models: models.clone(), context_tokens: b.context_tokens };
            match c.providers.iter_mut().find(|x| x.id == b.id) {
                Some(x) => *x = p,
                None => c.providers.push(p),
            }
            if c.agent.default_model.is_empty() || c.resolve_model(&c.agent.default_model).is_none() {
                c.agent.default_model = format!("{}/{}", b.id, models[0]);
            }
            Ok(())
        })
        .map_err(bad)?;
    if let Some(k) = b.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        app.update_secrets(|s| s.set(&format!("provider.{}.api_key", b.id), k)).map_err(bad)?;
    }
    app.emit(json!({ "type": "config.updated" }));
    Ok(Json(json!({ "ok": true, "default": cfg.agent.default_model, "models": cfg.all_models() })))
}

async fn remove_provider(State(app): State<AppRef>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let cfg = app
        .update_config(|c| {
            let before = c.providers.len();
            c.providers.retain(|p| p.id != id);
            if c.providers.len() == before {
                return Err(format!("no such provider: {id}"));
            }
            if c.resolve_model(&c.agent.default_model).is_none() {
                c.agent.default_model = c.all_models().into_iter().next().unwrap_or_default();
            }
            Ok(())
        })
        .map_err(bad)?;
    app.update_secrets(|s| s.remove_prefix(&format!("provider.{id}."))).map_err(bad)?;
    app.emit(json!({ "type": "config.updated" }));
    Ok(Json(json!({ "ok": true, "default": cfg.agent.default_model })))
}

// ---------------------------------------------------------------- secrets

fn secret_allowed(name: &str) -> bool {
    // server.token is managed by `alice setup` only.
    let fixed = ["telegram.bot_token", "discord.bot_token", "google.client_secret", "brave.api_key"];
    fixed.contains(&name)
        || name
            .strip_prefix("provider.")
            .and_then(|r| r.strip_suffix(".api_key"))
            .is_some_and(valid_ident)
}

#[derive(Deserialize)]
struct SecretBody {
    value: String,
}

async fn set_secret(State(app): State<AppRef>, Path(name): Path<String>, Json(b): Json<SecretBody>) -> ApiResult<Json<Value>> {
    if !secret_allowed(&name) {
        return Err(bad("unknown secret name"));
    }
    app.update_secrets(|s| s.set(&name, b.value.trim())).map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_secret(State(app): State<AppRef>, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    if !secret_allowed(&name) && name != "google.refresh_token" {
        return Err(bad("unknown secret name"));
    }
    app.update_secrets(|s| s.set(&name, "")).map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- memory

#[derive(Deserialize)]
struct MemQuery {
    q: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_memories(State(app): State<AppRef>, Query(q): Query<MemQuery>) -> ApiResult<Json<Value>> {
    let c = app.db.lock();
    let limit = q.limit.unwrap_or(100).min(1000);
    let list = match q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => {
            let mut hits = memory::search(&c, text, limit)?;
            if hits.is_empty() {
                // Short queries (< 3 chars) cannot use the trigram index.
                let like = format!("%{}%", text.replace('%', "\\%").replace('_', "\\_"));
                let mut st = c.prepare(
                    "SELECT m.id, m.key, m.content, m.created_at, m.updated_at,
                     (SELECT COUNT(*) FROM memory_sources s WHERE s.memory_id = m.id)
                     FROM memories m WHERE m.content LIKE ?1 ESCAPE '\\' OR m.key LIKE ?1 ESCAPE '\\' LIMIT ?2",
                )?;
                hits = st
                    .query_map(rusqlite::params![like, limit as i64], |r| {
                        Ok(memory::Memory { id: r.get(0)?, key: r.get(1)?, content: r.get(2)?, created_at: r.get(3)?, updated_at: r.get(4)?, source_count: r.get(5)? })
                    })?
                    .collect::<Result<_, _>>()?;
            }
            hits
        }
        None => memory::list(&c, limit, q.offset.unwrap_or(0))?,
    };
    Ok(Json(json!({ "memories": list })))
}

async fn memory_status(State(app): State<AppRef>) -> ApiResult<Json<Value>> {
    let s = memory::status(&app.db.lock(), &app.paths)?;
    let cfg = app.config();
    Ok(Json(json!({ "status": s, "auto_extract": cfg.memory.auto_extract, "inject_limit": cfg.memory.inject_limit })))
}

async fn get_memory(State(app): State<AppRef>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let (m, sources) = memory::get(&app.db.lock(), id)?.ok_or_else(not_found)?;
    Ok(Json(json!({ "memory": m, "sources": sources })))
}

async fn clear_memories(State(app): State<AppRef>, Json(b): Json<ConfirmBody>) -> ApiResult<StatusCode> {
    if b.confirm != "clear-memories" {
        return Err(bad("send {\"confirm\":\"clear-memories\"}"));
    }
    memory::clear(&mut app.db.lock())?;
    app.emit(json!({ "type": "memory.updated", "cleared": true }));
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- config / tools

fn public_config(app: &App) -> Value {
    let cfg = app.config();
    let mut v = serde_json::to_value(&cfg).unwrap_or_default();
    let secrets = app.secrets();
    v["secrets_set"] = json!(secrets.names().filter(|n| *n != "server.token").collect::<Vec<_>>());
    v["version"] = json!(env!("CARGO_PKG_VERSION"));
    v
}

async fn get_config(State(app): State<AppRef>) -> Json<Value> {
    Json(public_config(&app))
}

/// Body: `{ "agent.name": "Alice", "server.port": 7878, ... }`
async fn patch_config(State(app): State<AppRef>, Json(b): Json<serde_json::Map<String, Value>>) -> ApiResult<Json<Value>> {
    app.update_config(|c| {
        for (k, v) in &b {
            if k.starts_with("tools.permissions") {
                return Err("use /api/tools to change permissions".into());
            }
            let raw = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            c.set_key(k, &raw)?;
        }
        Ok(())
    })
    .map_err(bad)?;
    app.emit(json!({ "type": "config.updated" }));
    Ok(Json(public_config(&app)))
}

async fn list_tools(State(app): State<AppRef>) -> Json<Value> {
    let cfg = app.config();
    let roots: Vec<String> = tools::fs::Sandbox::new(&cfg, &app.paths).roots().iter().map(|p| p.display().to_string()).collect();
    Json(json!({ "tools": tools::list_json(&cfg, &app), "fs_roots": roots }))
}

/// Body: `{ "fs_delete": "ask", "web": "allow", ... }` (tool or group names).
/// Only reachable with the user's API token; never exposed as a tool.
async fn patch_tools(State(app): State<AppRef>, Json(b): Json<serde_json::Map<String, Value>>) -> ApiResult<Json<Value>> {
    let groups = ["fs", "web", "google"];
    app.update_config(|c| {
        for (k, v) in &b {
            if tools::find(k).is_none() && !groups.contains(&k.as_str()) {
                return Err(format!("unknown tool: {k}"));
            }
            match v {
                Value::Null => {
                    c.tools.permissions.remove(k);
                }
                Value::String(s) if s == "default" => {
                    c.tools.permissions.remove(k);
                }
                Value::String(s) => {
                    let p = Permission::parse(s).ok_or_else(|| format!("invalid permission: {s}"))?;
                    c.tools.permissions.insert(k.clone(), p);
                }
                _ => return Err(format!("invalid permission for {k}")),
            }
        }
        Ok(())
    })
    .map_err(bad)?;
    let cfg = app.config();
    Ok(Json(json!({ "tools": tools::list_json(&cfg, &app) })))
}

/// Bind and serve until shutdown.
pub async fn serve(app: AppRef) -> std::io::Result<()> {
    let cfg = app.config();
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let loopback = cfg.server.bind == "127.0.0.1" || cfg.server.bind == "::1" || cfg.server.bind == "localhost";
    if !loopback {
        crate::warn!("listening on non-loopback address {addr}: use Tailscale or an HTTPS reverse proxy for remote access");
    }
    crate::info!("alice {} listening on http://{addr}", env!("CARGO_PKG_VERSION"));
    let router = router(app.clone());
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            {
                let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal");
                tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
            }
            #[cfg(not(unix))]
            let _ = ctrl_c.await;
            crate::info!("shutting down");
        })
        .await?;
    // Leave the DB checkpointed on exit.
    let _ = storage::checkpoint(&app.db.lock());
    Ok(())
}
