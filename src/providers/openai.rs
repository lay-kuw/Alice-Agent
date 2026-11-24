//! OpenAI Chat Completions (also OpenAI-compatible servers and Ollama `/v1`).

use super::*;
use crate::config::{ProviderConfig, ProviderKind};
use serde_json::{Value, json};

pub fn build_body(kind: ProviderKind, req: &ChatRequest) -> Value {
    let mut messages = Vec::with_capacity(req.messages.len() + 1);
    if !req.system.is_empty() {
        messages.push(json!({ "role": "system", "content": req.system }));
    }
    for m in &req.messages {
        match m.role {
            Role::User => {
                if m.images.is_empty() {
                    messages.push(json!({ "role": "user", "content": m.content }));
                } else {
                    let mut parts = vec![json!({ "type": "text", "text": m.content })];
                    for img in &m.images {
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": { "url": format!("data:{};base64,{}", img.mime, img.data_b64) }
                        }));
                    }
                    messages.push(json!({ "role": "user", "content": parts }));
                }
            }
            Role::Assistant => {
                let mut msg = json!({ "role": "assistant", "content": m.content });
                if !m.tool_calls.is_empty() {
                    msg["tool_calls"] = m
                        .tool_calls
                        .iter()
                        .map(|c| {
                            json!({
                                "id": c.id, "type": "function",
                                "function": { "name": c.name, "arguments": c.arguments.to_string() }
                            })
                        })
                        .collect();
                }
                messages.push(msg);
            }
            Role::Tool => messages.push(json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.as_deref().unwrap_or(""),
                "content": m.content,
            })),
        }
    }
    let mut body = json!({ "model": req.model, "messages": messages, "stream": true });
    if kind == ProviderKind::Openai {
        body["max_completion_tokens"] = json!(req.max_tokens);
        body["stream_options"] = json!({ "include_usage": true });
    } else {
        body["max_tokens"] = json!(req.max_tokens);
    }
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| {
                json!({ "type": "function", "function": {
                    "name": t.name, "description": t.description, "parameters": t.parameters
                }})
            })
            .collect();
    }
    body
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

pub async fn stream(
    http: &reqwest::Client,
    p: &ProviderConfig,
    api_key: Option<&str>,
    req: &ChatRequest<'_>,
    on_delta: OnDelta<'_>,
) -> Result<Completion> {
    let url = format!("{}/chat/completions", p.base_url().trim_end_matches('/'));
    let mut rb = http.post(url).json(&build_body(p.kind, req));
    if let Some(k) = api_key {
        rb = rb.bearer_auth(k);
    }
    let resp = check_status(rb.send().await?).await?;
    let mut sse = SseLines::new(resp);
    let mut out = Completion::default();
    let mut calls: Vec<PartialCall> = Vec::new();

    while let Some(data) = sse.next_data().await? {
        if data == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
        if let Some(err) = v.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("provider error");
            return Err(ProviderError(msg.to_string()));
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            out.usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(0);
            out.usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0);
        }
        let Some(choice) = v.pointer("/choices/0") else { continue };
        if let Some(r) = choice["finish_reason"].as_str() {
            out.stop_reason = r.to_string();
        }
        let delta = &choice["delta"];
        if let Some(t) = delta["content"].as_str()
            && !t.is_empty() {
                out.text.push_str(t);
                on_delta(t);
            }
        if let Some(tcs) = delta["tool_calls"].as_array() {
            for tc in tcs {
                let idx = tc["index"].as_u64().unwrap_or(calls.len() as u64) as usize;
                while calls.len() <= idx {
                    calls.push(PartialCall::default());
                }
                let c = &mut calls[idx];
                if let Some(id) = tc["id"].as_str() {
                    c.id = id.to_string();
                }
                if let Some(n) = tc.pointer("/function/name").and_then(|n| n.as_str()) {
                    c.name.push_str(n);
                }
                if let Some(a) = tc.pointer("/function/arguments").and_then(|a| a.as_str()) {
                    c.args.push_str(a);
                }
            }
        }
    }
    out.tool_calls = calls
        .into_iter()
        .filter(|c| !c.name.is_empty())
        .map(|c| ToolCall {
            id: if c.id.is_empty() { crate::util::new_id() } else { c.id },
            name: c.name,
            arguments: parse_args(&c.args),
        })
        .collect();
    Ok(out)
}
