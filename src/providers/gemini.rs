//! Google Gemini API (`streamGenerateContent`, SSE).
//!
//! Function-call parts carry a `thoughtSignature` that must be echoed back
//! within the same turn; it is kept in `provider_state`.

use super::*;
use crate::config::ProviderConfig;
use serde_json::{Value, json};

fn parts_for(m: &ChatMessage) -> Vec<Value> {
    match m.role {
        Role::User => {
            let mut parts: Vec<Value> = m
                .images
                .iter()
                .map(|i| json!({ "inlineData": { "mimeType": i.mime, "data": i.data_b64 } }))
                .collect();
            if !m.content.is_empty() {
                parts.push(json!({ "text": m.content }));
            }
            parts
        }
        Role::Assistant => {
            if let Some(p) = m.provider_state.as_ref().and_then(|s| s.get("gemini_parts")).and_then(|p| p.as_array()) {
                return p.clone();
            }
            let mut parts = Vec::new();
            if !m.content.is_empty() {
                parts.push(json!({ "text": m.content }));
            }
            for c in &m.tool_calls {
                parts.push(json!({ "functionCall": { "name": c.name, "args": c.arguments } }));
            }
            parts
        }
        Role::Tool => {
            let response = serde_json::from_str::<Value>(&m.content)
                .ok()
                .filter(|v| v.is_object())
                .unwrap_or_else(|| json!({ "result": m.content }));
            vec![json!({ "functionResponse": {
                "name": m.tool_name.as_deref().unwrap_or("tool"),
                "response": response,
            }})]
        }
    }
}

pub fn build_body(req: &ChatRequest) -> Value {
    let mut contents: Vec<Value> = Vec::new();
    for m in &req.messages {
        let role = if m.role == Role::Assistant { "model" } else { "user" };
        let parts = parts_for(m);
        if parts.is_empty() {
            continue;
        }
        match contents.last_mut() {
            Some(last) if last["role"] == role => last["parts"].as_array_mut().unwrap().extend(parts),
            _ => contents.push(json!({ "role": role, "parts": parts })),
        }
    }
    let mut body = json!({
        "contents": contents,
        "generationConfig": { "maxOutputTokens": req.max_tokens },
    });
    if !req.system.is_empty() {
        body["systemInstruction"] = json!({ "parts": [{ "text": req.system }] });
    }
    if !req.tools.is_empty() {
        let decls: Vec<Value> = req
            .tools
            .iter()
            .map(|t| json!({ "name": t.name, "description": t.description, "parameters": t.parameters }))
            .collect();
        body["tools"] = json!([{ "functionDeclarations": decls }]);
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
    let url = format!(
        "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
        p.base_url().trim_end_matches('/'),
        req.model
    );
    let resp = http
        .post(url)
        .header("x-goog-api-key", api_key.unwrap_or(""))
        .json(&build_body(req))
        .send()
        .await?;
    let resp = check_status(resp).await?;
    let mut sse = SseLines::new(resp);
    let mut out = Completion::default();
    let mut model_parts: Vec<Value> = Vec::new();
    let mut has_signature = false;

    while let Some(data) = sse.next_data().await? {
        let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
        if let Some(err) = v.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("provider error");
            return Err(ProviderError(msg.to_string()));
        }
        if let Some(u) = v.get("usageMetadata") {
            out.usage.input_tokens = u["promptTokenCount"].as_u64().unwrap_or(out.usage.input_tokens);
            out.usage.output_tokens = u["candidatesTokenCount"].as_u64().unwrap_or(out.usage.output_tokens);
        }
        let Some(cand) = v.pointer("/candidates/0") else { continue };
        if let Some(r) = cand["finishReason"].as_str() {
            out.stop_reason = r.to_string();
        }
        let Some(parts) = cand.pointer("/content/parts").and_then(|p| p.as_array()) else { continue };
        for part in parts {
            if part.get("thoughtSignature").is_some() {
                has_signature = true;
            }
            if part["thought"].as_bool() == Some(true) {
                model_parts.push(part.clone());
                continue;
            }
            if let Some(t) = part["text"].as_str() {
                out.text.push_str(t);
                on_delta(t);
            }
            if let Some(fc) = part.get("functionCall") {
                out.tool_calls.push(ToolCall {
                    id: fc["id"].as_str().map(str::to_string).unwrap_or_else(crate::util::new_id),
                    name: fc["name"].as_str().unwrap_or("").to_string(),
                    arguments: fc.get("args").cloned().unwrap_or(json!({})),
                });
            }
            model_parts.push(part.clone());
        }
    }
    if has_signature {
        out.provider_state = Some(json!({ "gemini_parts": model_parts }));
    }
    Ok(out)
}
