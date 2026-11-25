//! HTTP client for the Alice server API.

use crate::config::{Config, Paths, Secrets};
use crate::providers::SseLines;
use serde_json::Value;

/// URL of the local server from its bind address and port.
/// The server always listens on loopback too (see `server::listen_addrs`).
pub fn local_url(cfg: &Config) -> String {
    let addrs = crate::server::listen_addrs(&cfg.server.bind);
    let host = if addrs.iter().any(|a| a == "::1" || a == "::") && !addrs.iter().any(|a| a == "127.0.0.1" || a == "0.0.0.0" || a == "localhost") { "[::1]" } else { "127.0.0.1" };
    format!("http://{host}:{}", cfg.server.port)
}

#[derive(Clone)]
pub struct Client {
    pub base: String,
    token: String,
    http: reqwest::Client,
}

pub type CliResult<T> = Result<T, String>;

impl Client {
    /// `ALICE_URL` / `ALICE_TOKEN` override the local config (remote CLI use).
    pub fn from_env(paths: &Paths) -> CliResult<Client> {
        let cfg = Config::load(paths)?;
        let base = std::env::var("ALICE_URL").unwrap_or_else(|_| local_url(&cfg));
        let token = match std::env::var("ALICE_TOKEN") {
            Ok(t) => t,
            Err(_) => Secrets::load(paths)?
                .get("server.token")
                .map(str::to_string)
                .ok_or("no API token; run `alice setup`")?,
        };
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Client { base: base.trim_end_matches('/').to_string(), token, http })
    }

    pub fn new(base: &str, token: &str) -> Client {
        let http = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(3)).build().expect("http client");
        Client { base: base.trim_end_matches('/').to_string(), token: token.to_string(), http }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api{}", self.base, path)
    }

    pub async fn is_up(&self) -> bool {
        self.http
            .get(self.url("/health"))
            .timeout(std::time::Duration::from_millis(800))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn handle(resp: reqwest::Response) -> CliResult<Value> {
        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        let v: Value = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) };
        if status.is_success() {
            Ok(v)
        } else {
            let msg = v.get("error").and_then(|e| e.as_str()).map(str::to_string).unwrap_or_else(|| v.to_string());
            Err(match status.as_u16() {
                401 => "unauthorized: the API token does not match the server (re-run `alice setup` or set ALICE_TOKEN)".into(),
                _ => msg,
            })
        }
    }

    fn conn_err(&self, e: reqwest::Error) -> String {
        if e.is_connect() || e.is_timeout() {
            format!("cannot reach the Alice server at {} (run `alice start`)", self.base)
        } else {
            e.without_url().to_string()
        }
    }

    pub async fn get(&self, path: &str) -> CliResult<Value> {
        let r = self.http.get(self.url(path)).bearer_auth(&self.token).send().await.map_err(|e| self.conn_err(e))?;
        Self::handle(r).await
    }

    pub async fn send(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> CliResult<Value> {
        let mut rb = self.http.request(method, self.url(path)).bearer_auth(&self.token);
        if let Some(b) = body {
            rb = rb.json(b);
        }
        let r = rb.send().await.map_err(|e| self.conn_err(e))?;
        Self::handle(r).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> CliResult<Value> {
        self.send(reqwest::Method::POST, path, Some(body)).await
    }
    pub async fn patch(&self, path: &str, body: &Value) -> CliResult<Value> {
        self.send(reqwest::Method::PATCH, path, Some(body)).await
    }
    pub async fn put(&self, path: &str, body: &Value) -> CliResult<Value> {
        self.send(reqwest::Method::PUT, path, Some(body)).await
    }
    pub async fn delete(&self, path: &str, body: Option<&Value>) -> CliResult<Value> {
        self.send(reqwest::Method::DELETE, path, body).await
    }

    pub async fn upload(&self, thread_id: &str, name: &str, bytes: Vec<u8>) -> CliResult<Value> {
        let r = self
            .http
            .post(self.url(&format!("/threads/{thread_id}/attachments")))
            .query(&[("name", name)])
            .bearer_auth(&self.token)
            .body(bytes)
            .send()
            .await
            .map_err(|e| self.conn_err(e))?;
        Self::handle(r).await
    }

    /// Open the SSE stream. Returns after the server's `hello` event.
    pub async fn events(&self, thread: Option<&str>) -> CliResult<EventStream> {
        let mut rb = self.http.get(self.url("/events")).bearer_auth(&self.token);
        if let Some(t) = thread {
            rb = rb.query(&[("thread", t)]);
        }
        let r = rb.send().await.map_err(|e| self.conn_err(e))?;
        if !r.status().is_success() {
            return Err(Self::handle(r).await.err().unwrap_or_default());
        }
        let mut es = EventStream { lines: SseLines::new(r) };
        // Wait for hello so no event emitted after this call is missed.
        es.next().await?;
        Ok(es)
    }
}

pub struct EventStream {
    lines: SseLines,
}

impl EventStream {
    pub async fn next(&mut self) -> CliResult<Value> {
        loop {
            match self.lines.next_data().await.map_err(|e| e.0)? {
                Some(d) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&d) {
                        return Ok(v);
                    }
                }
                None => return Err("event stream closed by server".into()),
            }
        }
    }
}
