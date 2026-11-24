//! Anthropic Messages API (raw HTTP, streaming).
//!
//! Thinking runs with the model's default (adaptive on current models); no
//! sampling parameters are sent. Thinking blocks are kept in
//! `provider_state` and replayed unchanged only within the turn that
//! produced them; earlier turns are sent as text + tool calls, which is
//! always valid even though the memory block in `system` changes per turn.

use super::*;
use crate::config::ProviderConfig;
use serde_json::{Value, json};

const API_VERSION: &str = "2023-06-01";

fn content_blocks(m: &ChatMessage) -> Vec<Value> {
    match m.role {
        Role::User => {
            let mut blocks: Vec<Value> = m
                .images
                .iter()
                .map(|i| json!({ "type": "image", "source": { "type": "base64", "media_type": i.mime, "data": i.data_b64 } }))
                .collect();
            if !m.content.is_empty() {
                blocks.push(json!({ "type": "text", "text": m.content }));
            }
            blocks
        }
        Role::Assistant => {
            if let Some(blocks) = m.provider_state.as_ref().and_then(|s| s.get("anthropic_blocks")).and_then(|b| b.as_array()) {
                return blocks.clone();
            }
            let mut blocks = Vec::new();
            if !m.content.is_empty() {
                blocks.push(json!({ "type": "text", "text": m.content }));
            }
            for c in &m.tool_calls {
                blocks.push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments }));
            }
            blocks
        }
        Role::Tool => vec![json!({
            "type": "tool_result",
            "tool_use_id": m.tool_call_id.as_deref().unwrap_or(""),
            "content": m.content,
        })],
    }
}

pub fn build_body(req: &ChatRequest) -> Value {
    // Merge consecutive same-role messages (tool results are user-role).
    let mut messages: Vec<Value> = Vec::new();
    for m in &req.messages {
        let role = if m.role == Role::Assistant { "assistant" } else { "user" };
        let blocks = content_blocks(m);
        if blocks.is_empty() {
            continue;
        }
        match messages.last_mut() {
            Some(last) if last["role"] == role => {
                last["content"].as_array_mut().unwrap().extend(blocks);
            }
            _ => messages.push(json!({ "role": role, "content": blocks })),
        }
    }
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "messages": messages,
        "stream": true,
    });
    if !req.system.is_empty() {
        body["system"] = json!(req.system);
    }
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters }))
            .collect();
    }
    body
}

pub async fn stream(
    http: &reqwest::Client,
    p: &ProviderConfig,
    api_key: Option<&str>,
    req: &ChatRequest<'_>,
    on_delta: OnDelta<'_>,
) -> Result<Completion> {
    let url = format!("{}/v1/messages", p.base_url().trim_end_matches('/'));
    let resp = http
        .post(url)
        .header("x-api-key", api_key.unwrap_or(""))
        .header("anthropic-version", API_VERSION)
        .json(&build_body(req))
        .send()
        .await?;
    let resp = check_status(resp).await?;
    let mut sse = SseLines::new(resp);
    let mut out = Completion::default();
    let mut blocks: Vec<Value> = Vec::new();
    let mut partial_json: Vec<String> = Vec::new();

    while let Some(data) = sse.next_data().await? {
        let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
        match v["type"].as_str().unwrap_or("") {
            "message_start" => {
                out.usage.input_tokens = v.pointer("/message/usage/input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
            }
            "content_block_start" => {
                let idx = v["index"].as_u64().unwrap_or(blocks.len() as u64) as usize;
                while blocks.len() <= idx {
                    blocks.push(Value::Null);
                    partial_json.push(String::new());
                }
                blocks[idx] = v["content_block"].clone();
            }
            "content_block_delta" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                let Some(block) = blocks.get_mut(idx) else { continue };
                let d = &v["delta"];
                match d["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or("");
                        let cur = block["text"].as_str().unwrap_or("").to_string() + t;
                        block["text"] = json!(cur);
                        on_delta(t);
                    }
                    "input_json_delta" => partial_json[idx].push_str(d["partial_json"].as_str().unwrap_or("")),
                    "thinking_delta" => {
                        let cur = block["thinking"].as_str().unwrap_or("").to_string() + d["thinking"].as_str().unwrap_or("");
                        block["thinking"] = json!(cur);
                    }
                    "signature_delta" => {
                        let cur = block["signature"].as_str().unwrap_or("").to_string() + d["signature"].as_str().unwrap_or("");
                        block["signature"] = json!(cur);
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                if let Some(block) = blocks.get_mut(idx)
                    && block["type"] == "tool_use" {
                        block["input"] = parse_args(&partial_json[idx]);
                    }
            }
            "message_delta" => {
                if let Some(r) = v.pointer("/delta/stop_reason").and_then(|r| r.as_str()) {
                    out.stop_reason = r.to_string();
                }
                if let Some(n) = v.pointer("/usage/output_tokens").and_then(|n| n.as_u64()) {
                    out.usage.output_tokens = n;
                }
            }
            "error" => {
                let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("provider error");
                return Err(ProviderError(msg.to_string()));
            }
            _ => {}
        }
    }

    blocks.retain(|b| !b.is_null());
    let mut has_thinking = false;
    for b in &blocks {
        match b["type"].as_str().unwrap_or("") {
            "text" => out.text.push_str(b["text"].as_str().unwrap_or("")),
            "tool_use" => out.tool_calls.push(ToolCall {
                id: b["id"].as_str().unwrap_or("").to_string(),
                name: b["name"].as_str().unwrap_or("").to_string(),
                arguments: b["input"].clone(),
            }),
            "thinking" | "redacted_thinking" => has_thinking = true,
            _ => {}
        }
    }
    if out.stop_reason == "refusal" && out.text.is_empty() {
        out.text = "(The model declined to answer this request.)".into();
    }
    if has_thinking {
        out.provider_state = Some(json!({ "anthropic_blocks": blocks }));
    }
    Ok(out)
}
