//! Provider adapters against local servers replaying each API's documented
//! streaming format (no real API calls).

use alice::config::{ProviderConfig, ProviderKind};
use alice::providers::{self, ChatMessage, ChatRequest, Role, ToolCall, ToolSpec};
use axum::Router;
use axum::response::IntoResponse;
use axum::routing::post;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

async fn serve(path: &'static str, body: String, seen: Arc<Mutex<Vec<(axum::http::HeaderMap, Value)>>>) -> String {
    let router = Router::new().route(
        path,
        post(move |headers: axum::http::HeaderMap, axum::Json(v): axum::Json<Value>| {
            let body = body.clone();
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push((headers, v));
                ([("content-type", "text/event-stream")], body).into_response()
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    url
}

fn tool() -> ToolSpec {
    ToolSpec { name: "fs_list".into(), description: "list".into(), parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}) }
}

#[tokio::test]
async fn anthropic_stream_text_thinking_and_tool_use() {
    let events = [
        json!({"type":"message_start","message":{"usage":{"input_tokens":12}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig123"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Let me "}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"look."}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"fs_list","input":{}}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"pa"}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"th\":\".\"}"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":30}}),
        json!({"type":"message_stop"}),
    ];
    let body: String = events.iter().map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap())).collect();
    let seen = Arc::new(Mutex::new(vec![]));
    let url = serve("/v1/messages", body, seen.clone()).await;
    let p = ProviderConfig { id: "a".into(), kind: ProviderKind::Anthropic, base_url: url, models: vec![], context_tokens: 0 };

    let mut tool_msg = ChatMessage::user("result text");
    tool_msg.role = Role::Tool;
    tool_msg.tool_call_id = Some("toolu_0".into());
    let mut prev = ChatMessage::assistant("");
    prev.tool_calls = vec![ToolCall { id: "toolu_0".into(), name: "fs_list".into(), arguments: json!({"path":"a"}) }];
    let req = ChatRequest {
        model: "claude-opus-5-5",
        system: "sys".into(),
        messages: vec![ChatMessage::user("hi"), prev, tool_msg, ChatMessage::user("again")],
        tools: vec![tool()],
        max_tokens: 1000,
        mcp: None,
        effort: None,
        tool_runner: None,
    };
    let mut text = String::new();
    let mut on = |d: &str| text.push_str(d);
    let c = providers::stream(&reqwest::Client::new(), &p, Some("k-test"), &req, &mut on).await.unwrap();
    assert_eq!(text, "Let me look.");
    assert_eq!(c.text, "Let me look.");
    assert_eq!(c.tool_calls, vec![ToolCall { id: "toolu_1".into(), name: "fs_list".into(), arguments: json!({"path":"."}) }]);
    assert_eq!(c.usage.input_tokens, 12);
    assert_eq!(c.usage.output_tokens, 30);
    let blocks = c.provider_state.unwrap()["anthropic_blocks"].clone();
    assert_eq!(blocks[0]["signature"], "sig123");

    let (headers, sent) = seen.lock().unwrap()[0].clone();
    assert_eq!(headers["x-api-key"], "k-test");
    assert_eq!(headers["anthropic-version"], "2023-06-01");
    assert_eq!(sent["system"], "sys");
    assert!(sent.get("temperature").is_none() && sent.get("thinking").is_none());
    // tool_result merged into a user turn; roles alternate.
    let roles: Vec<&str> = sent["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, vec!["user", "assistant", "user"]);
    assert_eq!(sent["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(sent["tools"][0]["input_schema"]["type"], "object");

    // Replay within a turn sends the exact blocks (thinking included).
    let mut replay = ChatMessage::assistant("Let me look.");
    replay.provider_state = Some(json!({ "anthropic_blocks": blocks }));
    let body = providers::anthropic::build_body(&ChatRequest { model: "m", system: String::new(), messages: vec![ChatMessage::user("q"), replay], tools: vec![], max_tokens: 10, mcp: None, effort: None, tool_runner: None });
    assert_eq!(body["messages"][1]["content"][0]["type"], "thinking");
}

#[tokio::test]
async fn gemini_stream_text_and_function_call_with_signature() {
    let chunks = [
        json!({"candidates":[{"content":{"role":"model","parts":[{"text":"Hel"}]}}]}),
        json!({"candidates":[{"content":{"role":"model","parts":[{"text":"lo"}]}}]}),
        json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"fs_list","args":{"path":"."}},"thoughtSignature":"ts1"}]},"finishReason":"STOP"}],
               "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":7}}),
    ];
    let body: String = chunks.iter().map(|c| format!("data: {c}\r\n\r\n")).collect();
    let seen = Arc::new(Mutex::new(vec![]));
    let url = serve("/v1beta/models/{model}", body, seen.clone()).await;
    let p = ProviderConfig { id: "g".into(), kind: ProviderKind::Gemini, base_url: url, models: vec![], context_tokens: 0 };
    let req = ChatRequest { model: "gemini-x", system: "sys".into(), messages: vec![ChatMessage::user("hi")], tools: vec![tool()], max_tokens: 100, mcp: None, effort: None, tool_runner: None };
    let mut text = String::new();
    let mut on = |d: &str| text.push_str(d);
    let c = providers::stream(&reqwest::Client::new(), &p, Some("gk"), &req, &mut on).await.unwrap();
    assert_eq!(text, "Hello");
    assert_eq!(c.tool_calls.len(), 1);
    assert_eq!(c.tool_calls[0].arguments, json!({"path":"."}));
    assert_eq!(c.usage.output_tokens, 7);
    let parts = c.provider_state.unwrap()["gemini_parts"].clone();
    assert_eq!(parts[2]["thoughtSignature"], "ts1");
    let (headers, sent) = seen.lock().unwrap()[0].clone();
    assert_eq!(headers["x-goog-api-key"], "gk");
    assert_eq!(sent["systemInstruction"]["parts"][0]["text"], "sys");
    assert_eq!(sent["tools"][0]["functionDeclarations"][0]["name"], "fs_list");
}

#[tokio::test]
async fn http_errors_are_reported_without_secrets() {
    let router = Router::new().route(
        "/v1/chat/completions",
        post(|| async { (axum::http::StatusCode::UNAUTHORIZED, axum::Json(json!({"error":{"message":"Incorrect API key"}}))) }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    let p = ProviderConfig { id: "o".into(), kind: ProviderKind::Openai, base_url: url, models: vec![], context_tokens: 0 };
    let req = ChatRequest { model: "x", system: String::new(), messages: vec![ChatMessage::user("hi")], tools: vec![], max_tokens: 10, mcp: None, effort: None, tool_runner: None };
    let mut on = |_: &str| {};
    let e = providers::stream(&reqwest::Client::new(), &p, Some("sk-SECRET"), &req, &mut on).await.unwrap_err();
    assert_eq!(e.0, "HTTP 401: Incorrect API key");
    // Missing key is caught before any request.
    let e = providers::stream(&reqwest::Client::new(), &p, None, &req, &mut on).await.unwrap_err();
    assert!(e.0.contains("no API key"));
}

#[cfg(unix)]
#[tokio::test]
async fn codex_cli_provider_uses_official_cli_output() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("codex");
    // Fake `codex`: records its arguments and stdin, prints documented JSONL events.
    let script = format!(
        "#!/bin/sh\n[ \"$1\" = app-server ] && exit 1\necho \"$@\" > {d}/args\ncat > {d}/stdin\n\
echo '{{\"type\":\"thread.started\",\"thread_id\":\"t\"}}'\n\
echo '{{\"type\":\"item.completed\",\"item\":{{\"id\":\"i1\",\"type\":\"reasoning\",\"text\":\"thinking\"}}}}'\n\
echo '{{\"type\":\"item.completed\",\"item\":{{\"id\":\"i2\",\"type\":\"agent_message\",\"text\":\"Hello from Codex\"}}}}'\n\
echo '{{\"type\":\"turn.completed\",\"usage\":{{\"input_tokens\":10,\"cached_input_tokens\":0,\"output_tokens\":4,\"reasoning_output_tokens\":0}}}}'\n",
        d = dir.path().display()
    );
    std::fs::write(&exe, script).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = ProviderConfig { id: "c".into(), kind: ProviderKind::CodexCli, base_url: exe.display().to_string(), models: vec![], context_tokens: 0 };
    let req = ChatRequest {
        model: "default",
        system: "be nice".into(),
        messages: vec![ChatMessage::user("first"), ChatMessage::assistant("answer"), ChatMessage::user("second")],
        tools: vec![],
        max_tokens: 100,
        mcp: None,
        effort: None,
        tool_runner: None,
    };
    let mut text = String::new();
    let mut on = |d: &str| text.push_str(d);
    let c = providers::stream(&reqwest::Client::new(), &p, None, &req, &mut on).await.unwrap();
    assert_eq!(c.text, "Hello from Codex");
    assert_eq!(c.usage.output_tokens, 4);
    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    for a in ["exec", "--json", "--ephemeral", "--sandbox read-only", "--skip-git-repo-check"] {
        assert!(args.contains(a), "{args}");
    }
    assert!(!args.contains("model_reasoning_effort") && !args.contains("-m "), "{args}");
    let stdin = std::fs::read_to_string(dir.path().join("stdin")).unwrap();
    assert!(stdin.contains("be nice") && stdin.contains("[User]\nfirst") && stdin.contains("[Assistant]\nanswer") && stdin.contains("second"));

    // Model and reasoning effort are passed to Codex.
    let req2 = ChatRequest { model: "gpt-6-luna", effort: Some("high".into()), messages: vec![ChatMessage::user("hi")], ..req };
    providers::stream(&reqwest::Client::new(), &p, None, &req2, &mut on).await.unwrap();
    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    assert!(args.contains("-m gpt-6-luna") && args.contains("model_reasoning_effort=\"high\""), "{args}");
    let req = req2;

    // A failing CLI reports its error.
    std::fs::write(&exe, "#!/bin/sh\n[ \"$1\" = app-server ] && exit 1\ncat >/dev/null\necho '{\"type\":\"turn.failed\",\"error\":{\"message\":\"not logged in\"}}'\nexit 1\n").unwrap();
    let e = providers::stream(&reqwest::Client::new(), &p, None, &req, &mut on).await.unwrap_err();
    assert_eq!(e.0, "not logged in");
}

#[tokio::test]
async fn gemini_models_are_discovered() {
    use axum::{Json, routing::get};
    let router = Router::new().route(
        "/v1beta/models",
        get(|h: axum::http::HeaderMap| async move {
            assert_eq!(h["x-goog-api-key"], "k");
            Json(json!({ "models": [
                { "name": "models/gemini-9-pro", "displayName": "Gemini 9 Pro", "description": "smart", "supportedGenerationMethods": ["generateContent", "countTokens"] },
                { "name": "models/gemini-embedding-9", "supportedGenerationMethods": ["embedContent"] },
                { "name": "models/gemini-9-flash-tts", "supportedGenerationMethods": ["generateContent"] } ] }))
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    let p = ProviderConfig { id: "g".into(), kind: ProviderKind::Gemini, base_url: url, models: vec![], context_tokens: 0 };
    let list = providers::catalog::discover(&reqwest::Client::new(), &p, Some("k")).await.unwrap().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].model.as_str(), list[0].name.as_str()), ("gemini-9-pro", "Gemini 9 Pro"));
    assert!(providers::catalog::discover(&reqwest::Client::new(), &p, None).await.is_err());
}


/// Fake `codex app-server`: answers initialize / thread/start / turn/start,
/// asks for one tool call, then streams the reply.
#[cfg(unix)]
#[tokio::test]
async fn codex_app_server_streams_and_calls_tools() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("codex");
    let script = format!(
        r#"#!/bin/sh
case "$1" in
  debug) echo '{{"tools":[{{"type":"function","name":"wait","description":"x"}}]}}'; exit 0;;
  app-server) ;;
  *) exit 1;;
esac
while read -r l; do
  printf '%s\n' "$l" >> {d}/in
  id=$(printf '%s' "$l" | sed -nE 's/^\{{"id":([0-9]+),.*/\1/p')
  case "$l" in
    *'"method":"initialize"'*) echo "{{\"id\":$id,\"result\":{{}}}}";;
    *'"method":"thread/start"'*) echo "{{\"id\":$id,\"result\":{{\"thread\":{{\"id\":\"t1\"}}}}}}";;
    *'"method":"turn/start"'*)
      echo "{{\"id\":$id,\"result\":{{\"turn\":{{\"id\":\"u1\"}}}}}}"
      echo '{{"id":"r1","method":"item/tool/call","params":{{"threadId":"t1","turnId":"u1","callId":"c1","tool":"fs_list","arguments":{{"path":"."}}}}}}';;
    *'"id":"r1"'*)
      echo '{{"method":"item/agentMessage/delta","params":{{"threadId":"t1","itemId":"m1","delta":"Hello "}}}}'
      echo '{{"method":"item/agentMessage/delta","params":{{"threadId":"t1","itemId":"m1","delta":"there"}}}}'
      echo '{{"method":"thread/tokenUsage/updated","params":{{"threadId":"t1","tokenUsage":{{"last":{{"inputTokens":7,"outputTokens":2}}}}}}}}'
      echo '{{"method":"turn/completed","params":{{"threadId":"t1","turn":{{"id":"u1","status":"completed"}}}}}}';;
    *'"id":'*) echo "{{\"id\":$id,\"result\":{{}}}}";;
  esac
done
"#,
        d = dir.path().display()
    );
    std::fs::write(&exe, script).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    alice::providers::cli::set_codex_home(dir.path().join("home"));
    let p = ProviderConfig { id: "c".into(), kind: ProviderKind::CodexCli, base_url: exe.display().to_string(), models: vec![], context_tokens: 0 };
    let calls = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let seen = calls.clone();
    let runner = providers::ToolRunner(Arc::new(move |name, args| {
        seen.lock().unwrap().push((name, args));
        Box::pin(async { ("a.txt\nb.txt".to_string(), true) })
    }));
    let req = ChatRequest {
        model: "gpt-6-luna",
        system: "be nice".into(),
        messages: vec![ChatMessage::user("list files")],
        tools: vec![ToolSpec { name: "fs_list".into(), description: "List files".into(), parameters: json!({"type": "object"}) }],
        max_tokens: 100,
        mcp: None,
        effort: Some("low".into()),
        tool_runner: Some(runner),
    };
    let mut deltas = Vec::new();
    let mut on = |d: &str| deltas.push(d.to_string());
    let c = providers::stream(&reqwest::Client::new(), &p, None, &req, &mut on).await.unwrap();
    assert_eq!(c.text, "Hello there");
    assert_eq!(deltas, vec!["Hello ", "there"], "streamed, not buffered");
    assert_eq!((c.usage.input_tokens, c.usage.output_tokens), (7, 2));
    assert_eq!(calls.lock().unwrap().as_slice(), &[("fs_list".to_string(), json!({"path": "."}))]);
    let sent = std::fs::read_to_string(dir.path().join("in")).unwrap();
    // Tools are offered natively; the result goes back to Codex; model and effort are set.
    assert!(sent.contains(r#""dynamicTools":[{"#) && sent.contains(r#""name":"fs_list""#), "{sent}");
    assert!(sent.contains(r#""id":"r1","result":{"contentItems":[{"text":"a.txt\nb.txt","type":"inputText"}],"success":true}"#), "{sent}");
    assert!(sent.contains(r#""effort":"low""#) && sent.contains(r#""model":"gpt-6-luna""#), "{sent}");
    assert!(sent.contains(r#""experimentalApi":true"#), "{sent}");
}
