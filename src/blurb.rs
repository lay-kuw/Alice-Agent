//! Alice's one-line comments at the top of the app screens ("2 unread mails,
//! I picked out the important ones ✨"). The app sends a few facts about the
//! screen; Alice answers in her voice (SOUL), in the tone set in
//! `agent.blurb_tone`. Answers are cached, so looking at a screen again costs
//! nothing until its facts change.

use crate::app::AppRef;
use crate::providers::{self, ChatMessage, ChatRequest};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const KEEP: Duration = Duration::from_secs(3 * 3600);
const TIMEOUT: Duration = Duration::from_secs(45);
pub const SCREENS: [&str; 5] = ["home", "chat", "calendar", "mail", "tasks"];

static CACHE: LazyLock<Mutex<HashMap<String, (Instant, String)>>> = LazyLock::new(Default::default);

fn what(screen: &str) -> &'static str {
    match screen {
        "home" => "the home screen, greeting the user and touching on their day",
        "chat" => "the chat screen, inviting the user to ask for something",
        "calendar" => "the calendar screen, about their upcoming events",
        "mail" => "the mail screen, about their inbox",
        _ => "the to-do screen, about their tasks",
    }
}

pub fn prompt(soul: &str, name: &str, tone: &str, screen: &str, lang: &str, now: &str) -> String {
    let soul = soul.trim().replace("{{name}}", name);
    let tone = if tone.trim().is_empty() { "your usual voice".to_string() } else { format!("this tone: {}", tone.trim()) };
    // "soul": whatever language her personality notes are written in.
    let language = match lang {
        "en" => "English",
        "ja" => "Japanese",
        _ => "the language your personality notes above are written in (Japanese if unclear)",
    };
    format!(
        "{soul}\n\nYou are writing one short line that {name} says at the top of {} in the user's app. \
Use {tone}. Write in {language}. At most two short sentences (about 50 Japanese characters or 20 English words), \
an emoji or two is fine, no markdown, no quotes. Base it on the facts given; if there is nothing notable, say something \
warm and brief. Never invent events, mails or tasks. Local time now: {now}. Output only the line.",
        what(screen)
    )
}

pub async fn say(app: &AppRef, screen: &str, lang: &str, facts: &str, now: &str) -> Result<String, String> {
    if !SCREENS.contains(&screen) {
        return Err("unknown screen".into());
    }
    let lang = match lang {
        "en" | "ja" => lang,
        _ => "soul",
    };
    let facts = crate::util::truncate(facts.trim(), 2000);
    let cfg = app.config();
    let key = format!("{screen}|{lang}|{}|{}|{facts}", cfg.agent.blurb_tone, crate::util::truncate(now, 13));
    if let Some((at, text)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&key)
        && at.elapsed() < KEEP {
            return Ok(text.clone());
        }
    let model = if cfg.memory.model.is_empty() { cfg.agent.default_model.as_str() } else { cfg.memory.model.as_str() };
    let (provider, model_name) = cfg.resolve_model(model).ok_or("no model configured")?;
    let api_key = app.secrets().provider_key(&provider.id).map(str::to_string);
    let req = ChatRequest {
        model: model_name,
        system: prompt(&crate::config::read_soul(&app.paths), &cfg.agent.name, &cfg.agent.blurb_tone, screen, lang, now),
        messages: vec![ChatMessage::user(format!("Facts:\n{}", if facts.is_empty() { "(nothing in particular)" } else { facts }))],
        tools: vec![],
        max_tokens: 300,
        mcp: None,
        effort: None,
        tool_runner: None,
    };
    let mut sink = |_: &str| {};
    let out = tokio::time::timeout(TIMEOUT, providers::stream(&app.http, provider, api_key.as_deref(), &req, &mut sink))
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let text = out.text.trim().trim_matches(|c| c == '"' || c == '「' || c == '」').trim().to_string();
    if text.is_empty() {
        return Err("empty".into());
    }
    let text = crate::util::truncate(&text, 400).to_string();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|_, (at, _)| at.elapsed() < KEEP);
    cache.insert(key, (Instant::now(), text.clone()));
    Ok(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn prompt_carries_soul_tone_and_language() {
        let p = super::prompt("You are {{name}}, cheerful.", "Alice", "a polite butler", "mail", "en", "2026-11-26 09:00");
        assert!(p.starts_with("You are Alice, cheerful."));
        assert!(p.contains("this tone: a polite butler") && p.contains("Write in English") && p.contains("inbox"));
        assert!(super::prompt("", "Alice", "", "tasks", "ja", "x").contains("your usual voice"));
    }
}
