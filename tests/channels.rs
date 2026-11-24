//! Telegram / Discord gateways against local fake platform APIs.

mod common;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use common::Harness;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct FakePlatform {
    updates: Mutex<Vec<Value>>,
    calls: Mutex<Vec<(String, Value)>>,
    next_id: Mutex<i64>,
}

impl FakePlatform {
    fn calls(&self, method: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(m, _)| m == method).map(|(_, v)| v.clone()).collect()
    }
    fn push(&self, u: Value) {
        self.updates.lock().unwrap().push(u);
    }
    async fn wait(&self, pred: impl Fn(&[(String, Value)]) -> bool) {
        for _ in 0..200 {
            if pred(&self.calls.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("condition not met; calls: {:#?}", self.calls.lock().unwrap());
    }
}

async fn telegram_method(State(st): State<Arc<FakePlatform>>, Path((bot, method)): Path<(String, String)>, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(bot, "bottg-token");
    st.calls.lock().unwrap().push((method.clone(), body.clone()));
    let result = match method.as_str() {
        "getMe" => json!({ "username": "alice_test_bot" }),
        "getUpdates" => {
            let u: Vec<Value> = std::mem::take(&mut *st.updates.lock().unwrap());
            if u.is_empty() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            json!(u)
        }
        "sendMessage" => {
            let mut n = st.next_id.lock().unwrap();
            *n += 1;
            json!({ "message_id": 1000 + *n })
        }
        _ => json!(true),
    };
    Json(json!({ "ok": true, "result": result }))
}

async fn serve(router: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    url
}

fn tg_message(update_id: i64, user: i64, text: &str) -> Value {
    json!({ "update_id": update_id, "message": { "message_id": update_id * 10, "from": { "id": user }, "chat": { "id": user }, "text": text } })
}

fn texts(calls: &[(String, Value)]) -> String {
    calls.iter().filter(|(m, _)| m == "sendMessage" || m == "editMessageText").map(|(_, v)| v["text"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn telegram_gateway_end_to_end() {
    let fake = Arc::new(FakePlatform::default());
    let url = serve(Router::new().route("/{bot}/{method}", post(telegram_method)).with_state(fake.clone())).await;
    let h = Harness::with(|c, s| {
        c.channels.telegram.enabled = true;
        c.channels.telegram.allowed_user_ids = vec![42];
        c.channels.telegram.api_base = url.clone();
        s.set("telegram.bot_token", "tg-token");
    })
    .await;
    alice::channels::start(&h.app);
    fake.wait(|c| c.iter().any(|(m, _)| m == "getUpdates")).await;

    // Strangers are ignored.
    fake.push(tg_message(1, 99, "hello from a stranger"));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(fake.calls("sendMessage").is_empty());
    assert_eq!(h.get("/status").await["counts"]["threads"], 0);

    // The owner chats; the answer streams back.
    fake.push(tg_message(2, 42, "I live in Osaka"));
    fake.wait(|c| texts(c).contains("ECHO=I live in Osaka")).await;
    let threads = h.get("/threads").await["threads"].as_array().unwrap().clone();
    assert_eq!(threads.len(), 1);
    let tid = threads[0]["id"].as_str().unwrap().to_string();
    let msgs = h.get(&format!("/threads/{tid}/messages")).await;
    assert_eq!(msgs["messages"][0]["client"], "telegram");

    // The same thread continues from the Web / CLI and back.
    h.chat(&tid, "continuing from the web", "web").await;
    fake.push(tg_message(3, 42, "and back on telegram"));
    fake.wait(|c| texts(c).contains("N=5")).await;

    // Commands.
    fake.push(tg_message(4, 42, "/threads"));
    fake.wait(|c| c.iter().any(|(m, v)| m == "sendMessage" && v["reply_markup"]["inline_keyboard"][0][0]["callback_data"].as_str().is_some_and(|d| d.starts_with("th:")))).await;
    fake.push(tg_message(5, 42, "/status"));
    fake.wait(|c| texts(c).contains("Threads: 1")).await;

    // Approval buttons: deny a destructive tool.
    std::fs::write(h.paths.workspace().join("victim.txt"), "x").unwrap();
    fake.push(tg_message(6, 42, "call fs_delete"));
    fake.wait(|c| c.iter().any(|(m, v)| m == "sendMessage" && v["reply_markup"].to_string().contains("ap:"))).await;
    let data = fake
        .calls("sendMessage")
        .iter()
        .find_map(|v| v["reply_markup"]["inline_keyboard"].as_array().and_then(|rows| rows.iter().filter_map(|r| r.as_array()).flatten().find_map(|b| b["callback_data"].as_str().filter(|d| d.ends_with(":n")).map(str::to_string))))
        .unwrap();
    fake.push(json!({ "update_id": 7, "callback_query": { "id": "cb1", "from": { "id": 42 }, "data": data, "message": { "message_id": 1, "chat": { "id": 42 }, "text": "Allow?" } } }));
    fake.wait(|c| c.iter().any(|(m, v)| m == "answerCallbackQuery" && v["text"] == "Denied")).await;
    fake.wait(|c| texts(c).contains("declined")).await;
    assert!(h.paths.workspace().join("victim.txt").exists());

    // Deleting the thread anywhere removes Alice's posted messages (best effort).
    assert_eq!(h.delete(&format!("/threads/{tid}"), None).await, 204);
    fake.wait(|c| c.iter().filter(|(m, _)| m == "deleteMessage").count() >= 3).await;

    // /new starts a fresh thread for the next message.
    fake.push(tg_message(8, 42, "/new"));
    fake.wait(|c| texts(c).contains("New thread")).await;
}

#[tokio::test]
async fn telegram_gateway_restarts_on_config_change() {
    let fake = Arc::new(FakePlatform::default());
    let url = serve(Router::new().route("/{bot}/{method}", post(telegram_method)).with_state(fake.clone())).await;
    let h = Harness::with(|c, s| {
        c.channels.telegram.allowed_user_ids = vec![42];
        c.channels.telegram.api_base = url.clone();
        s.set("telegram.bot_token", "tg-token");
    })
    .await;
    alice::channels::start(&h.app);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(fake.calls("getMe").is_empty(), "disabled gateway must not connect");
    let (code, _) = h.req(reqwest::Method::PATCH, "/config", Some(json!({ "channels.telegram.enabled": true }))).await;
    assert_eq!(code, 200);
    fake.wait(|c| c.iter().any(|(m, _)| m == "getMe")).await;
}

#[derive(Default)]
struct FakeDiscord {
    calls: Mutex<Vec<(String, String, Value)>>,
}

async fn discord_rest(State(st): State<Arc<FakeDiscord>>, method: axum::http::Method, uri: axum::http::Uri, body: Option<Json<Value>>) -> Json<Value> {
    let v = body.map(|b| b.0).unwrap_or(Value::Null);
    st.calls.lock().unwrap().push((method.to_string(), uri.path().to_string(), v));
    Json(json!({ "id": format!("m{}", st.calls.lock().unwrap().len()) }))
}

#[tokio::test]
async fn discord_dispatch_handles_dms_mentions_and_allowlist() {
    let fake = Arc::new(FakeDiscord::default());
    let url = serve(Router::new().fallback(discord_rest).route("/unused", get(|| async { "" })).with_state(fake.clone())).await;
    let h = Harness::with(|c, s| {
        c.channels.discord.allowed_user_ids = vec!["111".into()];
        c.channels.discord.api_base = url.clone();
        s.set("discord.bot_token", "dc-token");
    })
    .await;
    let dc = Arc::new(alice::channels::discord::Discord::new(&h.app, "dc-token"));
    alice::channels::discord::dispatch(&h.app, &dc, "READY", &json!({ "user": { "id": "999", "username": "alice" } })).await;

    let posted = |fake: &FakeDiscord| -> String {
        fake.calls.lock().unwrap().iter().filter(|(m, _, _)| m == "POST" || m == "PATCH").map(|(_, _, v)| v["content"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join("\n")
    };
    // Stranger: ignored.
    alice::channels::discord::dispatch(&h.app, &dc, "MESSAGE_CREATE", &json!({ "id": "1", "channel_id": "c1", "author": { "id": "222" }, "content": "hi" })).await;
    assert!(posted(&fake).is_empty());
    // Owner DM.
    alice::channels::discord::dispatch(&h.app, &dc, "MESSAGE_CREATE", &json!({ "id": "2", "channel_id": "c1", "author": { "id": "111" }, "content": "hello in dm" })).await;
    assert!(posted(&fake).contains("ECHO=hello in dm"), "{}", posted(&fake));
    // Guild message without mention: ignored; with mention: handled, mention stripped.
    alice::channels::discord::dispatch(&h.app, &dc, "MESSAGE_CREATE", &json!({ "id": "3", "guild_id": "g", "channel_id": "c2", "author": { "id": "111" }, "content": "no mention", "mentions": [] })).await;
    assert!(!posted(&fake).contains("no mention"));
    alice::channels::discord::dispatch(&h.app, &dc, "MESSAGE_CREATE", &json!({ "id": "4", "guild_id": "g", "channel_id": "c2", "author": { "id": "111" }, "content": "<@999> in guild", "mentions": [{ "id": "999" }] })).await;
    assert!(posted(&fake).contains("ECHO=in guild"));
    // Authorization header carries the bot token.
    let calls = fake.calls.lock().unwrap().clone();
    assert!(calls.iter().any(|(_, p, _)| p == "/channels/c1/messages"));
}

#[tokio::test]
async fn discord_gateway_session_identifies_and_dispatches() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as Ws;
    // Fake gateway WebSocket.
    let wsl = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_url = format!("ws://{}", wsl.local_addr().unwrap());
    let identified = Arc::new(Mutex::new(None::<Value>));
    let heartbeats = Arc::new(Mutex::new(0usize));
    {
        let (identified, heartbeats) = (identified.clone(), heartbeats.clone());
        tokio::spawn(async move {
            let (sock, _) = wsl.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            ws.send(Ws::text(json!({ "op": 10, "d": { "heartbeat_interval": 100 } }).to_string())).await.unwrap();
            while let Some(Ok(m)) = ws.next().await {
                let Ws::Text(t) = m else { continue };
                let v: Value = serde_json::from_str(&t).unwrap();
                match v["op"].as_u64().unwrap() {
                    2 => {
                        *identified.lock().unwrap() = Some(v);
                        ws.send(Ws::text(json!({ "op": 0, "s": 1, "t": "READY", "d": { "user": { "id": "999", "username": "alice" } } }).to_string())).await.unwrap();
                        ws.send(Ws::text(json!({ "op": 0, "s": 2, "t": "MESSAGE_CREATE", "d": { "id": "5", "channel_id": "c9", "author": { "id": "111" }, "content": "via gateway" } }).to_string())).await.unwrap();
                    }
                    1 => {
                        *heartbeats.lock().unwrap() += 1;
                        ws.send(Ws::text(json!({ "op": 11 }).to_string())).await.unwrap();
                    }
                    _ => {}
                }
            }
        });
    }
    let fake = Arc::new(FakeDiscord::default());
    let ws_for_rest = ws_url.clone();
    let rest = Router::new()
        .route("/gateway/bot", get(move || { let u = ws_for_rest.clone(); async move { Json(json!({ "url": u })) } }))
        .fallback(discord_rest)
        .with_state(fake.clone());
    let url = serve(rest).await;
    let h = Harness::with(|c, s| {
        c.channels.discord.enabled = true;
        c.channels.discord.allowed_user_ids = vec!["111".into()];
        c.channels.discord.api_base = url.clone();
        s.set("discord.bot_token", "dc-token");
    })
    .await;
    alice::channels::start(&h.app);
    for _ in 0..200 {
        let done = fake.calls.lock().unwrap().iter().any(|(_, _, v)| v["content"].as_str().is_some_and(|c| c.contains("ECHO=via gateway")));
        if done && *heartbeats.lock().unwrap() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let id = identified.lock().unwrap().clone().expect("identify sent");
    assert_eq!(id["d"]["token"], "dc-token");
    assert!(*heartbeats.lock().unwrap() > 0);
    assert!(fake.calls.lock().unwrap().iter().any(|(_, _, v)| v["content"].as_str().is_some_and(|c| c.contains("ECHO=via gateway"))));
}
