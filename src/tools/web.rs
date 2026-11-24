//! Web tools: search (Brave Search API or SearXNG) and URL fetch with
//! readable-text extraction. Fetched content is wrapped as untrusted data.
//!
//! SSRF guard: only http(s), and every hop (redirects followed manually)
//! must resolve to public addresses unless `tools.web.allow_private` is set.

use super::{ToolDef, arg_str, untrusted};
use crate::app::App;
use crate::config::{Config, Permission};
use serde_json::{Value, json};
use std::net::IpAddr;
use std::time::Duration;

const MAX_BODY: usize = 3 * 1024 * 1024;
const DEFAULT_CHARS: usize = 20_000;

pub static DEFS: [ToolDef; 2] = [
    ToolDef {
        name: "web_search",
        group: "web",
        default: Permission::Allow,
        description: "Search the web. Returns titles, URLs and snippets.",
        params: || json!({"type":"object","properties":{"query":{"type":"string"},"count":{"type":"integer","description":"1-10, default 5"}},"required":["query"]}),
    },
    ToolDef {
        name: "web_fetch",
        group: "web",
        default: Permission::Ask,
        description: "Fetch a public web page and return its readable text.",
        params: || json!({"type":"object","properties":{"url":{"type":"string"},"max_chars":{"type":"integer","description":"default 20000"}},"required":["url"]}),
    },
];

pub fn available(cfg: &Config, app: &App, name: &str) -> bool {
    match name {
        "web_search" => match cfg.tools.web.search_provider.as_str() {
            "brave" => app.secrets().get("brave.api_key").is_some(),
            "searxng" => !cfg.tools.web.searxng_url.is_empty(),
            _ => false,
        },
        _ => true,
    }
}

pub async fn execute(app: &App, cfg: &Config, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "web_search" => search(app, cfg, arg_str(args, "query")?, args.get("count").and_then(|v| v.as_u64()).unwrap_or(5).clamp(1, 10) as usize).await,
        "web_fetch" => {
            let max = args.get("max_chars").and_then(|v| v.as_u64()).map(|v| v as usize).unwrap_or(DEFAULT_CHARS).clamp(500, 200_000);
            fetch(cfg, arg_str(args, "url")?, max).await
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

async fn search(app: &App, cfg: &Config, query: &str, count: usize) -> Result<String, String> {
    let results: Vec<Value> = match cfg.tools.web.search_provider.as_str() {
        "brave" => {
            let key = app.secrets().get("brave.api_key").map(str::to_string).ok_or("Brave API key not set")?;
            let r = app
                .http
                .get("https://api.search.brave.com/res/v1/web/search")
                .query(&[("q", query), ("count", &count.to_string())])
                .header("X-Subscription-Token", key)
                .header("Accept", "application/json")
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| e.without_url().to_string())?;
            if !r.status().is_success() {
                return Err(format!("search failed: HTTP {}", r.status().as_u16()));
            }
            let v: Value = r.json().await.map_err(|e| e.to_string())?;
            v.pointer("/web/results")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().take(count).map(|x| json!({ "title": x["title"], "url": x["url"], "snippet": strip_tags(x["description"].as_str().unwrap_or("")) })).collect())
                .unwrap_or_default()
        }
        "searxng" => {
            let url = format!("{}/search", cfg.tools.web.searxng_url.trim_end_matches('/'));
            let r = app
                .http
                .get(url)
                .query(&[("q", query), ("format", "json")])
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| e.without_url().to_string())?;
            if !r.status().is_success() {
                return Err(format!("search failed: HTTP {} (is the JSON format enabled in SearXNG?)", r.status().as_u16()));
            }
            let v: Value = r.json().await.map_err(|e| e.to_string())?;
            v["results"]
                .as_array()
                .map(|a| a.iter().take(count).map(|x| json!({ "title": x["title"], "url": x["url"], "snippet": x["content"] })).collect())
                .unwrap_or_default()
        }
        _ => return Err("no search provider configured".into()),
    };
    Ok(untrusted(&format!("web_search:{query}"), &serde_json::to_string_pretty(&results).unwrap_or_default()))
}

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 (CGNAT, Tailscale)
                || (o[0] == 198 && (o[1] & 0xfe) == 18)) // benchmarking
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80) // link local
        }
    }
}

async fn check_url(cfg: &Config, url: &reqwest::Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only http and https URLs are allowed".into());
    }
    if cfg.tools.web.allow_private {
        return Ok(());
    }
    let host = url.host_str().ok_or("URL has no host")?;
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs: Vec<_> = tokio::net::lookup_host((host.trim_matches(['[', ']']), port))
        .await
        .map_err(|_| format!("cannot resolve {host}"))?
        .collect();
    if addrs.is_empty() || addrs.iter().any(|a| !is_public(a.ip())) {
        return Err(format!("{host} resolves to a private or local address; refusing to fetch"));
    }
    Ok(())
}

async fn fetch(cfg: &Config, raw: &str, max_chars: usize) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(25))
        .user_agent(concat!("Mozilla/5.0 (compatible; alice-agent/", env!("CARGO_PKG_VERSION"), ")"))
        .build()
        .map_err(|e| e.to_string())?;
    let mut url = reqwest::Url::parse(raw.trim()).map_err(|e| format!("invalid URL: {e}"))?;
    let mut resp = None;
    for _ in 0..6 {
        check_url(cfg, &url).await?;
        let r = client.get(url.clone()).send().await.map_err(|e| e.without_url().to_string())?;
        if r.status().is_redirection() {
            let loc = r.headers().get("location").and_then(|l| l.to_str().ok()).ok_or("redirect without location")?;
            url = url.join(loc).map_err(|e| e.to_string())?;
            continue;
        }
        resp = Some(r);
        break;
    }
    let mut r = resp.ok_or("too many redirects")?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    let ctype = r.headers().get("content-type").and_then(|c| c.to_str().ok()).unwrap_or("").to_lowercase();
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = r.chunk().await.map_err(|e| e.without_url().to_string())? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY {
            body.truncate(MAX_BODY);
            break;
        }
    }
    let text = String::from_utf8_lossy(&body);
    let (title, content) = if ctype.contains("html") || (ctype.is_empty() && text.trim_start().starts_with('<')) {
        extract(&text)
    } else if ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("xml") {
        (String::new(), text.into_owned())
    } else {
        return Err(format!("unsupported content type: {ctype}"));
    };
    let mut content = content;
    if content.len() > max_chars {
        content = format!("{}\n[truncated]", crate::util::truncate(&content, max_chars));
    }
    let header = if title.is_empty() { format!("URL: {url}\n\n") } else { format!("Title: {title}\nURL: {url}\n\n") };
    Ok(untrusted(&format!("web:{url}"), &(header + &content)))
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[..rest.len().min(12)].find(';');
        let Some(end) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match rep {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
}

/// Very small HTML-to-text extractor: drops non-content elements, keeps
/// block structure as line breaks, prefers <main>/<article> when present.
pub fn extract(html: &str) -> (String, String) {
    let lower = html.to_ascii_lowercase();
    let title = lower
        .find("<title")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .and_then(|s| lower[s..].find("</title>").map(|e| (s, s + e)))
        .map(|(s, e)| strip_tags(&html[s..e]).split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    // Prefer the main content region.
    let (mut start, mut end) = (0, html.len());
    for tag in ["<article", "<main"] {
        if let Some(i) = lower.find(tag) {
            let close = format!("</{}>", &tag[1..]);
            if let Some(j) = lower[i..].rfind(&close) {
                start = i;
                end = i + j;
                break;
            }
        }
    }
    let region = &html[start..end];
    let lregion = &lower[start..end];
    let skip = ["script", "style", "noscript", "svg", "head", "nav", "footer", "form", "iframe", "template"];
    let blocks = ["p", "div", "br", "li", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "section", "article", "pre", "blockquote", "table", "ul", "ol"];
    let mut out = String::new();
    let mut i = 0;
    let bytes = region.as_bytes();
    while i < region.len() {
        if bytes[i] == b'<' {
            let Some(close) = region[i..].find('>') else { break };
            let tag_raw = &lregion[i + 1..i + close];
            let name: String = tag_raw.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if !tag_raw.starts_with('/') && skip.contains(&name.as_str()) && !tag_raw.ends_with('/') {
                let end_tag = format!("</{name}");
                match lregion[i..].find(&end_tag) {
                    Some(e) => {
                        i += e;
                        i += region[i..].find('>').map(|x| x + 1).unwrap_or(region.len() - i);
                        continue;
                    }
                    None => break,
                }
            }
            if blocks.contains(&name.as_str()) {
                out.push('\n');
            }
            if name == "li" && !tag_raw.starts_with('/') {
                out.push_str("- ");
            }
            i += close + 1;
        } else {
            let next = region[i..].find('<').map(|n| i + n).unwrap_or(region.len());
            out.push_str(&region[i..next]);
            i = next;
        }
    }
    let text = decode_entities(&out);
    let mut lines: Vec<String> = Vec::new();
    for l in text.lines() {
        let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() {
            if lines.last().is_some_and(|x| !x.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.push(l);
        }
    }
    (title, lines.join("\n").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_readable_text() {
        let html = r#"<html><head><title>T &amp; X</title><style>.a{}</style></head><body><nav>menu</nav>
            <main><h1>Hello</h1><p>World &lt;3 &#x41;</p><script>evil()</script><ul><li>one</li><li>two</li></ul></main>
            <footer>foot</footer></body></html>"#;
        let (t, c) = extract(html);
        assert_eq!(t, "T & X");
        assert!(c.contains("Hello") && c.contains("World <3 A") && c.contains("- one"));
        assert!(!c.contains("evil") && !c.contains("menu") && !c.contains("foot"));
    }

    #[test]
    fn private_addresses_are_blocked() {
        for ip in ["127.0.0.1", "10.0.0.1", "192.168.1.1", "172.16.0.1", "169.254.169.254", "100.100.1.1", "::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1", "0.0.0.0"] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn refuses_local_targets() {
        let cfg = Config::default();
        for u in ["http://127.0.0.1:7878/api/config", "http://localhost/", "file:///etc/passwd", "http://[::1]/"] {
            assert!(fetch(&cfg, u, 1000).await.is_err(), "{u}");
        }
    }
}

#[cfg(test)]
mod live {
    /// Network test; run with `cargo test -- --ignored live_fetch`.
    #[tokio::test]
    #[ignore]
    async fn live_fetch() {
        let out = super::fetch(&crate::config::Config::default(), "http://example.com", 5000).await.unwrap();
        assert!(out.contains("Example Domain"), "{out}");
        println!("{out}");
    }
}
