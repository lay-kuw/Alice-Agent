//! Provider adapters. One request/response shape, one dispatch function.
//! The agent core never knows which provider it is talking to.

pub mod anthropic;
pub mod cli;
pub mod gemini;
pub mod openai;

use crate::config::{ProviderConfig, ProviderKind};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug)]
pub struct Image {
    pub mime: String,
    pub data_b64: String,
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub images: Vec<Image>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
    /// Tool name for tool results (Gemini needs it).
    pub tool_name: Option<String>,
    /// Provider replay data. Only set for messages produced in the current
    /// turn; earlier turns are sent without it (see ARCHITECTURE.md §4).
    pub provider_state: Option<Value>,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: text.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
            provider_state: None,
        }
    }
    pub fn assistant(text: impl Into<String>) -> ChatMessage {
        ChatMessage { role: Role::Assistant, ..ChatMessage::user(text) }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub provider_state: Option<Value>,
    pub usage: Usage,
    pub stop_reason: String,
}

#[derive(Debug)]
pub struct ProviderError(pub String);

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        // reqwest errors may include the URL; never include headers/keys.
        ProviderError(format!("request failed: {}", e.without_url()))
    }
}

pub type Result<T> = std::result::Result<T, ProviderError>;

/// Receives streamed text as it arrives.
pub type OnDelta<'a> = &'a mut (dyn FnMut(&str) + Send);

pub async fn stream(
    http: &reqwest::Client,
    provider: &ProviderConfig,
    api_key: Option<&str>,
    req: &ChatRequest<'_>,
    on_delta: OnDelta<'_>,
) -> Result<Completion> {
    if provider.kind.needs_api_key() && api_key.is_none() {
        return Err(ProviderError(format!(
            "no API key for provider '{}' (run `alice model login {}`)",
            provider.id, provider.id
        )));
    }
    match provider.kind {
        ProviderKind::Openai | ProviderKind::OpenaiCompatible | ProviderKind::Ollama => {
            openai::stream(http, provider, api_key, req, on_delta).await
        }
        ProviderKind::Anthropic => anthropic::stream(http, provider, api_key, req, on_delta).await,
        ProviderKind::Gemini => gemini::stream(http, provider, api_key, req, on_delta).await,
        ProviderKind::CodexCli | ProviderKind::GeminiCli => cli::stream(provider, req, on_delta).await,
    }
}

/// Static capability matrix (see ARCHITECTURE.md §4).
pub fn capabilities(kind: ProviderKind) -> Value {
    let (streaming, tools, image, file, reasoning, auth) = match kind {
        ProviderKind::Openai => ("yes", "yes", "yes", "as-text", "model-dependent", "api-key"),
        ProviderKind::OpenaiCompatible => ("yes", "server-dependent", "server-dependent", "as-text", "no", "api-key-optional"),
        ProviderKind::Ollama => ("yes", "model-dependent", "model-dependent", "as-text", "no", "none"),
        ProviderKind::Anthropic => ("yes", "yes", "yes", "as-text", "adaptive", "api-key"),
        ProviderKind::Gemini => ("yes", "yes", "yes", "as-text", "yes", "api-key"),
        ProviderKind::CodexCli => ("message", "no", "no", "no", "internal", "chatgpt-login (codex CLI)"),
        ProviderKind::GeminiCli => ("message", "no", "no", "no", "internal", "google-login (gemini CLI, experimental)"),
    };
    json!({
        "streaming": streaming, "tool_calling": tools, "image_input": image,
        "file_input": file, "reasoning": reasoning, "authentication": auth,
    })
}

pub fn supports_tools(kind: ProviderKind) -> bool {
    !matches!(kind, ProviderKind::CodexCli | ProviderKind::GeminiCli)
}

pub(crate) async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.pointer("/0/error/message"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| crate::util::truncate(&body, 300).to_string());
    Err(ProviderError(format!("HTTP {}: {}", status.as_u16(), msg)))
}

/// Minimal SSE reader yielding the payload of each `data:` line.
pub struct SseLines {
    inner: futures_util::stream::BoxStream<'static, reqwest::Result<axum::body::Bytes>>,
    buf: Vec<u8>,
}

impl SseLines {
    pub fn new(resp: reqwest::Response) -> SseLines {
        SseLines { inner: resp.bytes_stream().boxed(), buf: Vec::new() }
    }

    pub async fn next_data(&mut self) -> Result<Option<String>> {
        loop {
            if let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim_end_matches(['\r', '\n']);
                if let Some(data) = line.strip_prefix("data:") {
                    return Ok(Some(data.trim_start().to_string()));
                }
                continue;
            }
            match self.inner.next().await {
                Some(chunk) => self.buf.extend_from_slice(&chunk?),
                None => {
                    if self.buf.is_empty() {
                        return Ok(None);
                    }
                    self.buf.push(b'\n');
                }
            }
        }
    }
}

/// Parse tool-call arguments that arrive as a JSON string.
pub(crate) fn parse_args(s: &str) -> Value {
    if s.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str(s).unwrap_or_else(|_| json!({ "_invalid_json": s }))
}
