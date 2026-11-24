//! `/usage`: subscription limits reported by the official CLIs and the
//! tokens Alice has used, per model.

use crate::app::AppRef;
use crate::config::ProviderKind;
use crate::providers::cli;
use crate::storage;
use crate::util::now_ms;
use serde_json::{Value, json};

pub async fn collect(app: &AppRef) -> Value {
    let cfg = app.config();
    let subs = cfg.providers.iter().filter(|p| matches!(p.kind, ProviderKind::CodexCli | ProviderKind::GeminiCli)).map(|p| {
        let p = p.clone();
        async move {
            match p.kind {
                ProviderKind::CodexCli => match cli::codex_rate_limits(p.base_url()).await {
                    Ok(v) => json!({ "id": p.id, "kind": "codex-cli", "ok": true, "limits": cli::summarize_rate_limits(&v) }),
                    Err(e) => json!({ "id": p.id, "kind": "codex-cli", "ok": false, "error": e }),
                },
                _ => json!({ "id": p.id, "kind": p.kind.as_str(), "ok": false, "error": "the Gemini CLI does not report remaining quota" }),
            }
        }
    });
    let subs = futures_util::future::join_all(subs).await;
    let now = now_ms();
    let tokens = |since: i64| {
        let rows = storage::token_usage(&app.db.lock(), since).unwrap_or_default();
        rows.into_iter()
            .map(|(m, i, o, n)| json!({ "model": m, "input_tokens": i, "output_tokens": o, "responses": n }))
            .collect::<Vec<_>>()
    };
    json!({
        "subscriptions": subs,
        "tokens": { "day": tokens(now - 86_400_000), "month": tokens(now - 30 * 86_400_000) },
    })
}

/// "in 2h 13m" / "in 3d 4h".
pub fn until(ms: i64) -> String {
    let m = ((ms - now_ms()).max(0) / 60_000) as u64;
    match m {
        0 => "now".into(),
        1..=59 => format!("in {m}m"),
        60..=1439 => format!("in {}h {}m", m / 60, m % 60),
        _ => format!("in {}d {}h", m / 1440, (m % 1440) / 60),
    }
}

pub fn window_name(minutes: u64) -> String {
    match minutes {
        10080 => "weekly".into(),
        1440 => "daily".into(),
        m if m % 60 == 0 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

pub fn bar(percent: f64, width: usize) -> String {
    let filled = ((percent.clamp(0.0, 100.0) / 100.0) * width as f64).round() as usize;
    "█".repeat(filled) + &"░".repeat(width - filled)
}

pub fn compact(n: i64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.1}k", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// Windows of one subscription: (label, used percent, reset text).
pub fn windows(limits: &Value) -> Vec<(String, f64, String)> {
    ["primary", "secondary"]
        .iter()
        .filter_map(|k| {
            let w = &limits[*k];
            let used = w["used_percent"].as_f64()?;
            let name = window_name(w["window_minutes"].as_u64().unwrap_or(0));
            let reset = w["resets_at"].as_i64().map(until).unwrap_or_default();
            Some((name, used, reset))
        })
        .collect()
}

/// Plain-text report (CLI, Telegram, Discord).
pub fn render_text(v: &Value) -> String {
    let mut s = String::new();
    let subs = v["subscriptions"].as_array().cloned().unwrap_or_default();
    if subs.is_empty() {
        s.push_str("No subscription providers (codex-cli) configured.\n");
    }
    for sub in &subs {
        let plan = sub.pointer("/limits/plan").and_then(|p| p.as_str()).map(|p| format!(" ({p})")).unwrap_or_default();
        s.push_str(&format!("{}{}\n", sub["id"].as_str().unwrap_or(""), plan));
        if sub["ok"] != true {
            s.push_str(&format!("  unavailable: {}\n", sub["error"].as_str().unwrap_or("")));
            continue;
        }
        for (name, used, reset) in windows(&sub["limits"]) {
            s.push_str(&format!("  {name:<7} {} {:>3.0}% used · {:>3.0}% left · resets {reset}\n", bar(used, 12), used, 100.0 - used));
        }
        if let Some(n) = sub.pointer("/limits/reset_credits").and_then(|n| n.as_i64()).filter(|n| *n > 0) {
            s.push_str(&format!("  reset credits available: {n}\n"));
        }
    }
    for (label, key) in [("Last 24 hours", "day"), ("Last 30 days", "month")] {
        let rows = v["tokens"][key].as_array().cloned().unwrap_or_default();
        s.push_str(&format!("\n{label} (tokens used by Alice)\n"));
        if rows.is_empty() {
            s.push_str("  —\n");
        }
        for r in rows {
            s.push_str(&format!(
                "  {:<28} in {:>7} · out {:>7} · {} replies\n",
                r["model"].as_str().unwrap_or(""),
                compact(r["input_tokens"].as_i64().unwrap_or(0)),
                compact(r["output_tokens"].as_i64().unwrap_or(0)),
                r["responses"]
            ));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_report() {
        let reset = now_ms() + 2 * 3_600_000 + 5 * 60_000;
        let v = json!({
            "subscriptions": [{ "id": "codex-cli", "ok": true, "limits": {
                "plan": "plus", "primary": { "used_percent": 25, "window_minutes": 300, "resets_at": reset },
                "secondary": { "used_percent": 1, "window_minutes": 10080, "resets_at": reset } } }],
            "tokens": { "day": [{ "model": "codex-cli/default", "input_tokens": 12345, "output_tokens": 678, "responses": 3 }], "month": [] },
        });
        let t = render_text(&v);
        assert!(t.contains("codex-cli (plus)") && t.contains("5h") && t.contains("weekly"), "{t}");
        assert!(t.contains("75% left") && t.contains("resets in 2h"), "{t}");
        assert!(t.contains("12.3k") && t.contains("3 replies"), "{t}");
        assert_eq!(bar(50.0, 4), "██░░");
    }
}
