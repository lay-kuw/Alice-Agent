//! `alice chat` full-screen TUI. A thin client: all state comes from the
//! server API and its event stream.

use super::client::{CliResult, Client};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event as TermEvent, KeyCode,
    KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

// ------------------------------------------------------------------ palette

const PINK: Color = Color::Rgb(255, 143, 199);
const LAVENDER: Color = Color::Rgb(180, 162, 255);
const MINT: Color = Color::Rgb(126, 224, 195);
const PEACH: Color = Color::Rgb(255, 196, 150);
const TEXT: Color = Color::Rgb(232, 228, 245);
const MUTED: Color = Color::Rgb(140, 134, 165);
const FAINT: Color = Color::Rgb(90, 86, 110);
const RED: Color = Color::Rgb(255, 120, 140);
const BUBBLE: Color = Color::Rgb(58, 42, 72);
const CODE_BG: Color = Color::Rgb(36, 33, 52);
const SPINNER: [&str; 4] = ["✿", "❀", "✾", "❁"];

fn s(fg: Color) -> Style {
    Style::default().fg(fg)
}

// ------------------------------------------------------------------ state

enum Overlay {
    None,
    Threads(ListState),
    Models(ListState),
    Approval(Value),
    Help,
    ConfirmDelete,
    Rename(String),
}

struct App {
    client: Client,
    profile: String,
    threads: Vec<Value>,
    current: Option<String>,
    messages: Vec<Value>,
    generating: bool,
    models: Vec<String>,
    default_model: String,
    pending_model: Option<String>,
    input: Vec<char>,
    cursor: usize,
    scroll: usize, // lines from the bottom
    overlay: Overlay,
    notice: Option<(String, bool)>,
    spin: usize,
    pending_attachments: Vec<String>,
    quit: bool,
    connected: bool,
}

impl App {
    fn thread(&self) -> Option<&Value> {
        let id = self.current.as_deref()?;
        self.threads.iter().find(|t| t["id"] == id)
    }

    fn model(&self) -> String {
        self.thread()
            .and_then(|t| t["model"].as_str())
            .map(str::to_string)
            .or(self.pending_model.clone())
            .unwrap_or_else(|| self.default_model.clone())
    }

    fn note(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), false));
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), true));
    }

    async fn reload_threads(&mut self) {
        if let Ok(v) = self.client.get("/threads").await {
            self.threads = v["threads"].as_array().cloned().unwrap_or_default();
        }
    }

    async fn reload_models(&mut self) {
        if let Ok(v) = self.client.get("/models").await {
            self.models = v["models"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
            self.default_model = v["default"].as_str().unwrap_or("").to_string();
        }
    }

    async fn open(&mut self, id: &str) {
        match self.client.get(&format!("/threads/{id}/messages")).await {
            Ok(v) => {
                self.current = Some(id.to_string());
                self.messages = v["messages"].as_array().cloned().unwrap_or_default();
                self.generating = v["generating"] == true;
                self.scroll = 0;
            }
            Err(e) => self.error(e),
        }
    }

    fn new_chat(&mut self) {
        self.current = None;
        self.messages.clear();
        self.generating = false;
        self.scroll = 0;
        self.pending_attachments.clear();
    }

    async fn ensure_thread(&mut self) -> CliResult<String> {
        if let Some(id) = &self.current {
            return Ok(id.clone());
        }
        let v = self.client.post("/threads", &json!({ "model": self.pending_model })).await?;
        let t = v["thread"].clone();
        let id = t["id"].as_str().unwrap_or("").to_string();
        if !self.threads.iter().any(|x| x["id"] == t["id"]) {
            self.threads.insert(0, t);
        }
        self.current = Some(id.clone());
        Ok(id)
    }

    async fn send(&mut self) {
        let text: String = self.input.iter().collect();
        let text = text.trim().to_string();
        if text.is_empty() && self.pending_attachments.is_empty() {
            return;
        }
        if text.starts_with('/') && self.pending_attachments.is_empty() {
            self.input.clear();
            self.cursor = 0;
            self.command(&text).await;
            return;
        }
        if self.generating {
            self.note("応答中です（Esc で停止）");
            return;
        }
        let id = match self.ensure_thread().await {
            Ok(id) => id,
            Err(e) => return self.error(e),
        };
        let atts = std::mem::take(&mut self.pending_attachments);
        match self.client.post(&format!("/threads/{id}/messages"), &json!({ "content": text, "client": "cli", "attachments": atts })).await {
            Ok(v) => {
                self.input.clear();
                self.cursor = 0;
                self.scroll = 0;
                self.notice = None;
                for k in ["user_message", "assistant_message"] {
                    upsert(&mut self.messages, v[k].clone());
                }
                self.generating = true;
            }
            Err(e) => self.error(e),
        }
    }

    async fn command(&mut self, line: &str) {
        let (cmd, arg) = line.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((line, ""));
        match cmd {
            "/exit" | "/quit" | "/q" => self.quit = true,
            "/new" => self.new_chat(),
            "/threads" | "/list" => self.overlay = Overlay::Threads(ListState::default().with_selected(Some(0))),
            "/model" => {
                let sel = self.models.iter().position(|m| *m == self.model()).unwrap_or(0);
                self.overlay = Overlay::Models(ListState::default().with_selected(Some(sel)));
            }
            "/rename" if !arg.is_empty() => {
                if let Some(id) = self.current.clone()
                    && let Err(e) = self.client.patch(&format!("/threads/{id}"), &json!({ "title": arg })).await {
                        self.error(e);
                    }
            }
            "/rename" => {
                let t = self.thread().and_then(|t| t["title"].as_str()).unwrap_or("").to_string();
                self.overlay = Overlay::Rename(t);
            }
            "/delete" if self.current.is_some() => self.overlay = Overlay::ConfirmDelete,
            "/attach" => {
                let path = std::path::Path::new(arg);
                match std::fs::read(path) {
                    Ok(bytes) => match self.ensure_thread().await {
                        Ok(id) => {
                            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                            match self.client.upload(&id, name, bytes).await {
                                Ok(v) => {
                                    self.pending_attachments.push(v["id"].as_str().unwrap_or("").to_string());
                                    self.note(format!("📎 {name} を添付しました"));
                                }
                                Err(e) => self.error(e),
                            }
                        }
                        Err(e) => self.error(e),
                    },
                    Err(e) => self.error(format!("{arg}: {e}")),
                }
            }
            "/help" | "/?" => self.overlay = Overlay::Help,
            _ => self.error(format!("不明なコマンド: {cmd}（/help）")),
        }
    }

    async fn stop(&mut self) {
        if let (Some(id), true) = (self.current.clone(), self.generating) {
            let _ = self.client.post(&format!("/threads/{id}/stop"), &json!({})).await;
        }
    }

    async fn on_event(&mut self, v: Value) {
        let here = v["thread_id"].as_str().is_some_and(|t| Some(t) == self.current.as_deref());
        match v["type"].as_str().unwrap_or("") {
            "hello" => {
                self.connected = true;
                self.reload_threads().await;
                if let Some(id) = self.current.clone() {
                    self.open(&id).await;
                }
            }
            "resync" => {
                self.reload_threads().await;
                if let Some(id) = self.current.clone() {
                    self.open(&id).await;
                }
            }
            "thread.created" | "thread.updated" => {
                let t = v["thread"].clone();
                match self.threads.iter_mut().find(|x| x["id"] == t["id"]) {
                    Some(x) => *x = t,
                    None => self.threads.insert(0, t),
                }
                self.threads.sort_by_key(|t| std::cmp::Reverse(t["updated_at"].as_i64().unwrap_or(0)));
            }
            "thread.deleted" => {
                self.threads.retain(|t| t["id"] != v["thread_id"]);
                if here {
                    self.new_chat();
                    self.note("このスレッドは削除されました");
                }
            }
            "threads.cleared" => {
                self.threads.clear();
                self.new_chat();
            }
            "message.created" if here => {
                if v["message"]["status"] == "streaming" {
                    self.generating = true;
                }
                upsert(&mut self.messages, v["message"].clone());
            }
            "message.delta" if here => {
                let id = v["message_id"].clone();
                if let Some(m) = self.messages.iter_mut().find(|m| m["id"] == id) {
                    let cur = m["content"].as_str().unwrap_or("").to_string() + v["delta"].as_str().unwrap_or("");
                    m["content"] = json!(cur);
                }
            }
            "message.completed" if here => {
                let m = v["message"].clone();
                if ends_turn(&m) {
                    self.generating = false;
                }
                upsert(&mut self.messages, m);
            }
            "tool.started" | "tool.completed" if here => {
                if let Some(id) = self.current.clone()
                    && let Ok(r) = self.client.get(&format!("/threads/{id}/messages")).await {
                        self.messages = r["messages"].as_array().cloned().unwrap_or_default();
                    }
            }
            "tool.approval_required" => {
                if here || v["client"] == "cli" {
                    self.overlay = Overlay::Approval(v);
                } else {
                    self.note(format!("承認待ち: {}（別のスレッド）", v["tool"].as_str().unwrap_or("")));
                }
            }
            "tool.approval_resolved" => {
                if let Overlay::Approval(a) = &self.overlay
                    && a["approval_id"] == v["approval_id"] {
                        self.overlay = Overlay::None;
                    }
            }
            "config.updated" => self.reload_models().await,
            _ => {}
        }
    }

    async fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        // Overlays first.
        match &mut self.overlay {
            Overlay::None => {}
            Overlay::Help => {
                self.overlay = Overlay::None;
                return;
            }
            Overlay::Approval(a) => {
                let id = a["approval_id"].as_str().unwrap_or("").to_string();
                let decision = match k.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => Some(true),
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(false),
                    _ => None,
                };
                if let Some(ok) = decision {
                    self.overlay = Overlay::None;
                    let _ = self.client.post(&format!("/approvals/{id}"), &json!({ "approve": ok })).await;
                }
                return;
            }
            Overlay::ConfirmDelete => {
                if matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y'))
                    && let Some(id) = self.current.clone()
                        && let Err(e) = self.client.delete(&format!("/threads/{id}"), None).await {
                            self.error(e);
                        }
                self.overlay = Overlay::None;
                return;
            }
            Overlay::Rename(buf) => {
                match k.code {
                    KeyCode::Esc => self.overlay = Overlay::None,
                    KeyCode::Enter => {
                        let title = buf.clone();
                        self.overlay = Overlay::None;
                        if let Some(id) = self.current.clone()
                            && let Err(e) = self.client.patch(&format!("/threads/{id}"), &json!({ "title": title })).await {
                                self.error(e);
                            }
                    }
                    KeyCode::Backspace => {
                        buf.pop();
                    }
                    KeyCode::Char(c) if !ctrl => buf.push(c),
                    _ => {}
                }
                return;
            }
            Overlay::Threads(state) => {
                let n = self.threads.len();
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = Overlay::None,
                    KeyCode::Up | KeyCode::Char('k') => state.select(Some(state.selected().unwrap_or(0).saturating_sub(1))),
                    KeyCode::Down | KeyCode::Char('j') => state.select(Some((state.selected().unwrap_or(0) + 1).min(n.saturating_sub(1)))),
                    KeyCode::Char('n') => {
                        self.overlay = Overlay::None;
                        self.new_chat();
                    }
                    KeyCode::Enter => {
                        let id = state.selected().and_then(|i| self.threads.get(i)).and_then(|t| t["id"].as_str()).map(str::to_string);
                        self.overlay = Overlay::None;
                        if let Some(id) = id {
                            self.open(&id).await;
                        }
                    }
                    KeyCode::Char('d') => {
                        let id = state.selected().and_then(|i| self.threads.get(i)).and_then(|t| t["id"].as_str()).map(str::to_string);
                        if let Some(id) = id {
                            self.overlay = Overlay::None;
                            self.open(&id).await;
                            self.overlay = Overlay::ConfirmDelete;
                        }
                    }
                    _ => {}
                }
                return;
            }
            Overlay::Models(state) => {
                let n = self.models.len();
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = Overlay::None,
                    KeyCode::Up | KeyCode::Char('k') => state.select(Some(state.selected().unwrap_or(0).saturating_sub(1))),
                    KeyCode::Down | KeyCode::Char('j') => state.select(Some((state.selected().unwrap_or(0) + 1).min(n.saturating_sub(1)))),
                    KeyCode::Enter => {
                        let m = state.selected().and_then(|i| self.models.get(i)).cloned();
                        self.overlay = Overlay::None;
                        if let Some(m) = m {
                            match self.current.clone() {
                                Some(id) => {
                                    if let Err(e) = self.client.patch(&format!("/threads/{id}"), &json!({ "model": m })).await {
                                        self.error(e);
                                    }
                                }
                                None => self.pending_model = Some(m.clone()),
                            }
                            self.note(format!("モデル: {m}"));
                        }
                    }
                    _ => {}
                }
                return;
            }
        }
        match k.code {
            KeyCode::Char('c') if ctrl => {
                if self.generating {
                    self.stop().await;
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('d') if ctrl && self.input.is_empty() => self.quit = true,
            KeyCode::Char('t') if ctrl => self.overlay = Overlay::Threads(ListState::default().with_selected(Some(0))),
            KeyCode::Char('n') if ctrl => self.new_chat(),
            KeyCode::Char('o') if ctrl => {
                let sel = self.models.iter().position(|m| *m == self.model()).unwrap_or(0);
                self.overlay = Overlay::Models(ListState::default().with_selected(Some(sel)));
            }
            KeyCode::Char('j') if ctrl => self.insert('\n'),
            KeyCode::Char('a') if ctrl => self.cursor = line_start(&self.input, self.cursor),
            KeyCode::Char('e') if ctrl => self.cursor = line_end(&self.input, self.cursor),
            KeyCode::Char('u') if ctrl => {
                self.input.clear();
                self.cursor = 0;
            }
            KeyCode::Esc => {
                if self.generating {
                    self.stop().await;
                } else {
                    self.notice = None;
                }
            }
            KeyCode::Enter if alt || k.modifiers.contains(KeyModifiers::SHIFT) => self.insert('\n'),
            KeyCode::Enter => self.send().await,
            KeyCode::Char('?') if self.input.is_empty() => self.overlay = Overlay::Help,
            KeyCode::Char(c) if !ctrl => self.insert(c),
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.input.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.input.len() => {
                self.input.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.input.len()),
            KeyCode::Home => self.cursor = line_start(&self.input, self.cursor),
            KeyCode::End => self.cursor = line_end(&self.input, self.cursor),
            KeyCode::PageUp => self.scroll += 10,
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Up if self.input.is_empty() => self.scroll += 1,
            KeyCode::Down if self.input.is_empty() => self.scroll = self.scroll.saturating_sub(1),
            _ => {}
        }
    }

    fn insert(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += 1;
    }
}

fn line_start(buf: &[char], cur: usize) -> usize {
    buf[..cur].iter().rposition(|c| *c == '\n').map(|i| i + 1).unwrap_or(0)
}

fn line_end(buf: &[char], cur: usize) -> usize {
    buf[cur..].iter().position(|c| *c == '\n').map(|i| cur + i).unwrap_or(buf.len())
}

fn upsert(list: &mut Vec<Value>, m: Value) {
    match list.iter_mut().find(|x| x["id"] == m["id"]) {
        Some(x) => *x = m,
        None => list.push(m),
    }
}

fn ends_turn(m: &Value) -> bool {
    m["role"] == "assistant" && (m["status"] != "complete" || m["tool_calls"].as_array().is_none_or(|a| a.is_empty())) && m["status"] != "streaming"
}

// ------------------------------------------------------------------ text layout

/// Wrap styled spans to `width` display columns (CJK-aware).
fn wrap(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(4);
    let mut lines: Vec<Vec<Span<'static>>> = vec![vec![]];
    let mut col = 0;
    for span in spans {
        let style = span.style;
        let mut cur = String::new();
        for ch in span.content.chars() {
            let w = ch.width().unwrap_or(0);
            if col + w > width {
                if !cur.is_empty() {
                    lines.last_mut().unwrap().push(Span::styled(std::mem::take(&mut cur), style));
                }
                lines.push(vec![]);
                col = 0;
                if ch == ' ' {
                    continue;
                }
            }
            cur.push(ch);
            col += w;
        }
        if !cur.is_empty() {
            lines.last_mut().unwrap().push(Span::styled(cur, style));
        }
    }
    lines
}

/// Inline Markdown: **bold**, `code`, *italic*.
fn inline_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let (mut bold, mut code, mut italic) = (false, false, false);
    let chars: Vec<char> = text.chars().collect();
    let style = |bold: bool, code: bool, italic: bool| {
        let mut st = base;
        if bold {
            st = st.add_modifier(Modifier::BOLD).fg(PEACH);
        }
        if italic {
            st = st.add_modifier(Modifier::ITALIC);
        }
        if code {
            st = Style::default().fg(MINT).bg(CODE_BG);
        }
        st
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let toggle = if c == '`' {
            Some(0)
        } else if !code && c == '*' && chars.get(i + 1) == Some(&'*') {
            Some(1)
        } else if !code && c == '*' && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) && (italic || i == 0 || chars[i - 1].is_whitespace()) {
            Some(2)
        } else {
            None
        };
        if let Some(kind) = toggle {
            if !buf.is_empty() {
                out.push(Span::styled(std::mem::take(&mut buf), style(bold, code, italic)));
            }
            match kind {
                0 => code = !code,
                1 => {
                    bold = !bold;
                    i += 1;
                }
                _ => italic = !italic,
            }
        } else {
            buf.push(c);
        }
        i += 1;
    }
    if !buf.is_empty() {
        out.push(Span::styled(buf, style(bold, code, italic)));
    }
    out
}

/// Render Markdown into wrapped lines.
fn markdown(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut in_code = false;
    for raw in text.lines() {
        let t = raw.trim_start();
        if t.starts_with("```") {
            in_code = !in_code;
            let lang = t.trim_start_matches('`').trim();
            if in_code && !lang.is_empty() {
                out.push(Line::from(Span::styled(format!(" {lang} "), Style::default().fg(MUTED).bg(CODE_BG))));
            }
            continue;
        }
        if in_code {
            for l in wrap(vec![Span::styled(format!(" {raw}"), Style::default().fg(MINT).bg(CODE_BG))], width) {
                out.push(Line::from(l).style(Style::default().bg(CODE_BG)));
            }
            continue;
        }
        if t.is_empty() {
            out.push(Line::default());
            continue;
        }
        let (prefix, body, style): (Option<Span>, &str, Style) = if let Some(h) = t.strip_prefix("### ").or(t.strip_prefix("## ")).or(t.strip_prefix("# ")) {
            (None, h, s(LAVENDER).add_modifier(Modifier::BOLD))
        } else if let Some(li) = t.strip_prefix("- ").or(t.strip_prefix("* ")).or(t.strip_prefix("+ ")) {
            (Some(Span::styled("  • ", s(PINK))), li, s(TEXT))
        } else if let Some(q) = t.strip_prefix('>') {
            (Some(Span::styled("  │ ", s(FAINT))), q.trim_start(), s(MUTED).add_modifier(Modifier::ITALIC))
        } else if let Some((n, rest)) = t.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
            (Some(Span::styled(format!("  {n}. "), s(PINK))), rest, s(TEXT))
        } else {
            (None, t, s(TEXT))
        };
        let mut spans = Vec::new();
        let indent = prefix.as_ref().map(|p| p.content.width()).unwrap_or(0);
        if let Some(p) = prefix {
            spans.push(p);
        }
        spans.extend(inline_spans(body, style));
        for (i, l) in wrap(spans, width).into_iter().enumerate() {
            let mut l = l;
            if i > 0 && indent > 0 {
                l.insert(0, Span::raw(" ".repeat(indent)));
            }
            out.push(Line::from(l));
        }
    }
    out
}

fn arg_summary(v: &Value) -> String {
    match v {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| format!("{k}={}", crate::util::truncate(v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()).as_str(), 40)))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// All conversation lines for the given width.
fn conversation(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line> = Vec::new();
    let tool_ok = |id: &str| -> Option<bool> {
        app.messages
            .iter()
            .find(|m| m["role"] == "tool" && m["tool_call_id"] == id)
            .map(|m| m.pointer("/meta/ok").and_then(|o| o.as_bool()).unwrap_or(true))
    };
    for m in &app.messages {
        match m["role"].as_str().unwrap_or("") {
            "user" => {
                let max = (width * 3 / 4).max(10);
                let text = m["content"].as_str().unwrap_or("");
                let mut body: Vec<Vec<Span>> = Vec::new();
                for l in text.lines() {
                    body.extend(wrap(vec![Span::styled(l.to_string(), s(TEXT).bg(BUBBLE))], max - 2));
                }
                out.push(Line::from(Span::styled("you ", s(PINK).add_modifier(Modifier::BOLD))).alignment(Alignment::Right));
                // Pad every line to the bubble's width so it reads as one block.
                let bw = body.iter().map(|l| l.iter().map(|s| s.content.width()).sum::<usize>()).max().unwrap_or(0);
                for l in body {
                    let w: usize = l.iter().map(|s| s.content.width()).sum();
                    let mut spans = vec![Span::styled(" ", Style::default().bg(BUBBLE))];
                    spans.extend(l);
                    spans.push(Span::styled(" ".repeat(bw - w + 1), Style::default().bg(BUBBLE)));
                    out.push(Line::from(spans).alignment(Alignment::Right));
                }
                out.push(Line::default());
            }
            "assistant" => {
                let content = m["content"].as_str().unwrap_or("");
                let calls = m["tool_calls"].as_array().cloned().unwrap_or_default();
                let streaming = m["status"] == "streaming";
                if content.is_empty() && calls.is_empty() && !streaming && m["status"] == "complete" {
                    continue;
                }
                let mut head = vec![Span::styled("✿ Alice", s(LAVENDER).add_modifier(Modifier::BOLD))];
                if streaming {
                    head.push(Span::styled(format!(" {}", SPINNER[app.spin % 4]), s(PINK)));
                }
                out.push(Line::from(head));
                out.extend(markdown(content, width.saturating_sub(1)));
                for c in &calls {
                    let id = c["id"].as_str().unwrap_or("");
                    let (icon, col) = match tool_ok(id) {
                        None => ("⋯", MUTED),
                        Some(true) => ("✓", MINT),
                        Some(false) => ("✗", RED),
                    };
                    let line = format!(" {icon} {} {}", c["name"].as_str().unwrap_or(""), arg_summary(&c["arguments"]));
                    out.push(Line::from(Span::styled(crate::util::truncate(&line, width).to_string(), s(col))));
                }
                match m["status"].as_str().unwrap_or("") {
                    "error" => out.push(Line::from(Span::styled(
                        format!(" ⚠ {}", m.pointer("/meta/error").and_then(|e| e.as_str()).unwrap_or("error")),
                        s(RED),
                    ))),
                    "cancelled" => out.push(Line::from(Span::styled(" （停止しました）", s(MUTED)))),
                    "complete" if calls.is_empty() => {
                        let secs = m.pointer("/meta/provider_ms").and_then(|n| n.as_u64()).unwrap_or(0) as f64 / 1000.0;
                        out.push(Line::from(Span::styled(format!("{} · {secs:.1}s", m["model"].as_str().unwrap_or("")), s(FAINT))));
                    }
                    _ => {}
                }
                if ends_turn(m) || streaming {
                    out.push(Line::default());
                }
            }
            _ => {}
        }
    }
    out
}

// ------------------------------------------------------------------ drawing

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(4));
    let h = h.min(area.height.saturating_sub(2));
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn popup<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(s(PINK))
        .title(Span::styled(format!(" {title} "), s(PINK).add_modifier(Modifier::BOLD)))
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let input_text: String = app.input.iter().collect();
    let input_width = area.width.saturating_sub(6) as usize;
    let input_lines: usize = input_text.split('\n').map(|l| (l.width() / input_width.max(1)) + 1).sum::<usize>().clamp(1, 8);
    let [header, body, input, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(input_lines as u16 + 2),
        Constraint::Length(1),
    ])
    .areas(area);

    // Header
    let title = app.thread().and_then(|t| t["title"].as_str()).filter(|t| !t.is_empty()).unwrap_or("新しいチャット").to_string();
    let dot = if app.connected { Span::styled("●", s(MINT)) } else { Span::styled("●", s(RED)) };
    let mut right = vec![Span::styled(app.model(), s(MUTED))];
    if app.profile != "default" {
        right.push(Span::styled(format!(" · {}", app.profile), s(PEACH)));
    }
    right.push(Span::raw(" "));
    right.push(dot);
    let rw: usize = right.iter().map(|s| s.content.width()).sum();
    let left = vec![Span::styled(" ✿ Alice ", s(PINK).add_modifier(Modifier::BOLD)), Span::styled(title, s(TEXT))];
    f.render_widget(Paragraph::new(Line::from(left)), header);
    let rarea = Rect { x: header.x + header.width.saturating_sub(rw as u16 + 1), width: rw as u16 + 1, ..header };
    f.render_widget(Paragraph::new(Line::from(right)), rarea);

    // Conversation
    let inner = Rect { x: body.x + 2, width: body.width.saturating_sub(4), ..body };
    if app.messages.is_empty() {
        let h = inner.height / 2;
        let lines = vec![
            Line::default(),
            Line::from(Span::styled("✿", s(PINK).add_modifier(Modifier::BOLD))).alignment(Alignment::Center),
            Line::from(Span::styled("なにをお手伝いしましょう？", s(TEXT).add_modifier(Modifier::BOLD))).alignment(Alignment::Center),
            Line::from(Span::styled("Enter で送信 · Ctrl+T スレッド · Ctrl+O モデル · ? ヘルプ", s(MUTED))).alignment(Alignment::Center),
        ];
        f.render_widget(Paragraph::new(lines), Rect { y: inner.y + h.saturating_sub(3), height: 5.min(inner.height), ..inner });
    } else {
        let lines = conversation(app, inner.width as usize);
        let total = lines.len();
        let view = inner.height as usize;
        let max_scroll = total.saturating_sub(view);
        let scroll = app.scroll.min(max_scroll);
        let start = total.saturating_sub(view + scroll);
        let visible: Vec<Line> = lines.into_iter().skip(start).take(view).collect();
        f.render_widget(Paragraph::new(visible), inner);
        if scroll > 0 {
            let tag = Span::styled(format!(" ↓ {scroll} "), Style::default().fg(Color::Black).bg(LAVENDER));
            f.render_widget(Paragraph::new(Line::from(tag)).alignment(Alignment::Right), Rect { y: inner.y + inner.height - 1, height: 1, ..inner });
        }
    }

    // Input
    let border = if app.generating { LAVENDER } else { PINK };
    let mut label = if app.generating { format!(" {} 考え中… Esc で停止 ", SPINNER[app.spin % 4]) } else { " メッセージ ".to_string() };
    if !app.pending_attachments.is_empty() {
        label.push_str(&format!("· 📎 {} ", app.pending_attachments.len()));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(s(border))
        .title(Span::styled(label, s(border)));
    let in_inner = block.inner(input);
    f.render_widget(block, input);
    let mut ilines: Vec<Line> = Vec::new();
    let mut cursor_pos = (0u16, 0u16);
    let before: String = app.input[..app.cursor].iter().collect();
    let width = in_inner.width.saturating_sub(2).max(1) as usize;
    // Lay out input with soft wraps, tracking the cursor.
    let mut row = 0usize;
    let mut col = 0usize;
    let mut cur_line: Vec<Span> = vec![Span::styled("› ", s(PINK))];
    let mut idx = 0usize;
    let before_len = before.chars().count();
    for ch in input_text.chars() {
        if idx == before_len {
            cursor_pos = (col as u16, row as u16);
        }
        idx += 1;
        if ch == '\n' {
            ilines.push(Line::from(std::mem::take(&mut cur_line)));
            cur_line.push(Span::raw("  "));
            row += 1;
            col = 0;
            continue;
        }
        let w = ch.width().unwrap_or(0);
        if col + w > width {
            ilines.push(Line::from(std::mem::take(&mut cur_line)));
            cur_line.push(Span::raw("  "));
            row += 1;
            col = 0;
        }
        cur_line.push(Span::styled(ch.to_string(), s(TEXT)));
        col += w;
    }
    if idx == before_len {
        cursor_pos = (col as u16, row as u16);
    }
    ilines.push(Line::from(cur_line));
    if app.input.is_empty() {
        ilines = vec![Line::from(vec![Span::styled("› ", s(PINK)), Span::styled("Alice にメッセージ…", s(FAINT))])];
    }
    let skip = (cursor_pos.1 as usize + 1).saturating_sub(in_inner.height as usize);
    f.render_widget(Paragraph::new(ilines.into_iter().skip(skip).collect::<Vec<_>>()), in_inner);
    if matches!(app.overlay, Overlay::None) {
        f.set_cursor_position(Position::new(in_inner.x + 2 + cursor_pos.0, in_inner.y + cursor_pos.1 - skip as u16));
    }

    // Footer
    let foot = match &app.notice {
        Some((msg, true)) => Line::from(Span::styled(format!(" ⚠ {msg}"), s(RED))),
        Some((msg, false)) => Line::from(Span::styled(format!(" ✿ {msg}"), s(MINT))),
        None => Line::from(vec![
            Span::styled(" ⏎", s(PINK)),
            Span::styled(" 送信  ", s(MUTED)),
            Span::styled("⌥⏎", s(PINK)),
            Span::styled(" 改行  ", s(MUTED)),
            Span::styled("^T", s(PINK)),
            Span::styled(" スレッド  ", s(MUTED)),
            Span::styled("^N", s(PINK)),
            Span::styled(" 新規  ", s(MUTED)),
            Span::styled("^O", s(PINK)),
            Span::styled(" モデル  ", s(MUTED)),
            Span::styled("?", s(PINK)),
            Span::styled(" ヘルプ  ", s(MUTED)),
            Span::styled("^C", s(PINK)),
            Span::styled(" 終了", s(MUTED)),
        ]),
    };
    f.render_widget(Paragraph::new(foot), footer);

    // Overlays
    match &app.overlay {
        Overlay::None => {}
        Overlay::Threads(state) => {
            let r = centered(area, 64, 20);
            f.render_widget(Clear, r);
            let items: Vec<ListItem> = app
                .threads
                .iter()
                .map(|t| {
                    let title = t["title"].as_str().filter(|s| !s.is_empty()).unwrap_or("（無題）");
                    let when = crate::util::fmt_utc(t["updated_at"].as_i64().unwrap_or(0));
                    let mark = if Some(t["id"].as_str().unwrap_or("")) == app.current.as_deref() { "● " } else { "  " };
                    ListItem::new(Line::from(vec![
                        Span::styled(mark, s(PINK)),
                        Span::styled(crate::util::truncate(title, 40).to_string(), s(TEXT)),
                        Span::styled(format!("  {}", &when[5..16]), s(FAINT)),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(popup("スレッド  ⏎ 開く · n 新規 · d 削除 · Esc"))
                .highlight_style(Style::default().bg(BUBBLE).add_modifier(Modifier::BOLD))
                .highlight_symbol("✿ ");
            let mut st = *state;
            f.render_stateful_widget(list, r, &mut st);
        }
        Overlay::Models(state) => {
            let r = centered(area, 56, (app.models.len() as u16 + 2).clamp(4, 18));
            f.render_widget(Clear, r);
            let cur = app.model();
            let items: Vec<ListItem> = app
                .models
                .iter()
                .map(|m| {
                    let mut spans = vec![Span::styled(m.clone(), s(TEXT))];
                    if *m == cur {
                        spans.push(Span::styled("  ✓", s(MINT)));
                    }
                    if *m == app.default_model {
                        spans.push(Span::styled("  既定", s(FAINT)));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let list = List::new(items).block(popup("モデル  ⏎ 選択 · Esc")).highlight_style(Style::default().bg(BUBBLE)).highlight_symbol("✿ ");
            let mut st = *state;
            f.render_stateful_widget(list, r, &mut st);
        }
        Overlay::Approval(a) => {
            let r = centered(area, 70, 12);
            f.render_widget(Clear, r);
            let args = serde_json::to_string_pretty(&a["arguments"]).unwrap_or_default();
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("🔐 ", s(PEACH)),
                    Span::styled(a["tool"].as_str().unwrap_or("").to_string(), s(PEACH).add_modifier(Modifier::BOLD)),
                    Span::styled(" を実行してもいい？", s(TEXT)),
                ]),
                Line::default(),
            ];
            for l in args.lines().take(6) {
                lines.push(Line::from(Span::styled(l.to_string(), Style::default().fg(MINT).bg(CODE_BG))));
            }
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::styled(" y ", Style::default().fg(Color::Black).bg(MINT)),
                Span::styled(" 許可   ", s(TEXT)),
                Span::styled(" n ", Style::default().fg(Color::Black).bg(RED)),
                Span::styled(" 拒否", s(TEXT)),
            ]));
            f.render_widget(Paragraph::new(lines).block(popup("確認")), r);
        }
        Overlay::ConfirmDelete => {
            let r = centered(area, 54, 6);
            f.render_widget(Clear, r);
            let lines = vec![
                Line::from(Span::styled("このスレッドを削除しますか？", s(TEXT).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled("ここから覚えたことも一緒に消えます。", s(MUTED))),
                Line::from(vec![
                    Span::styled(" y ", Style::default().fg(Color::Black).bg(RED)),
                    Span::styled(" 削除   ", s(TEXT)),
                    Span::styled(" Esc ", Style::default().fg(Color::Black).bg(MUTED)),
                    Span::styled(" やめる", s(TEXT)),
                ]),
            ];
            f.render_widget(Paragraph::new(lines).block(popup("削除")), r);
        }
        Overlay::Rename(buf) => {
            let r = centered(area, 60, 3);
            f.render_widget(Clear, r);
            f.render_widget(Paragraph::new(Line::from(Span::styled(buf.clone(), s(TEXT)))).block(popup("名前を変更  ⏎ 保存 · Esc")), r);
            f.set_cursor_position(Position::new(r.x + 1 + buf.width() as u16, r.y + 1));
        }
        Overlay::Help => {
            let r = centered(area, 62, 19);
            f.render_widget(Clear, r);
            let row = |k: &str, d: &str| Line::from(vec![Span::styled(format!("  {k:<14}"), s(PINK)), Span::styled(d.to_string(), s(TEXT))]);
            let lines = vec![
                row("Enter", "送信"),
                row("Alt+Enter / ^J", "改行"),
                row("Esc", "応答を止める"),
                row("Ctrl+T", "スレッド一覧"),
                row("Ctrl+N", "新しいチャット"),
                row("Ctrl+O", "モデルを選ぶ"),
                row("PgUp / PgDn", "スクロール（ホイールでも）"),
                row("Ctrl+C", "停止 / 終了"),
                Line::default(),
                row("/rename 名前", "スレッド名を変更"),
                row("/delete", "スレッドを削除"),
                row("/attach パス", "ファイルを添付"),
                row("/new /threads", "新規 / 一覧"),
                row("/exit", "終了"),
                Line::default(),
                Line::from(Span::styled("  なにかキーを押すと閉じます", s(FAINT))),
            ];
            f.render_widget(Paragraph::new(lines).block(popup("ヘルプ")), r);
        }
    }
}

// ------------------------------------------------------------------ run

pub async fn run(client: &Client, thread: Option<String>, profile: &str) -> CliResult<()> {
    let mut app = App {
        client: client.clone(),
        profile: profile.to_string(),
        threads: vec![],
        current: None,
        messages: vec![],
        generating: false,
        models: vec![],
        default_model: String::new(),
        pending_model: None,
        input: vec![],
        cursor: 0,
        scroll: 0,
        overlay: Overlay::None,
        notice: None,
        spin: 0,
        pending_attachments: vec![],
        quit: false,
        connected: false,
    };
    // Fail early (before taking over the screen) if the server is unreachable.
    app.client.get("/threads").await?;
    app.reload_models().await;
    app.reload_threads().await;
    if let Some(id) = thread {
        app.open(&id).await;
    }

    // Server events on a task; terminal input on a thread.
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<Value>();
    let c2 = client.clone();
    let events_task = tokio::spawn(async move {
        loop {
            if let Ok(mut es) = c2.events(None).await {
                let _ = ev_tx.send(json!({ "type": "hello" }));
                while let Ok(v) = es.next().await {
                    if ev_tx.send(v).is_err() {
                        return;
                    }
                }
            }
            let _ = ev_tx.send(json!({ "type": "disconnected" }));
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    let (key_tx, mut key_rx) = mpsc::unbounded_channel::<TermEvent>();
    std::thread::spawn(move || {
        while let Ok(e) = event::read() {
            if key_tx.send(e).is_err() {
                break;
            }
        }
    });

    let mut terminal: DefaultTerminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    let result: CliResult<()> = async {
        let mut tick = tokio::time::interval(Duration::from_millis(120));
        loop {
            terminal.draw(|f| draw(f, &app)).map_err(|e| e.to_string())?;
            if app.quit {
                return Ok(());
            }
            tokio::select! {
                Some(e) = key_rx.recv() => match e {
                    TermEvent::Key(k) => app.on_key(k).await,
                    TermEvent::Paste(p) => {
                        for c in p.replace("\r\n", "\n").chars() {
                            app.insert(c);
                        }
                    }
                    TermEvent::Mouse(m) => match m.kind {
                        MouseEventKind::ScrollUp => app.scroll += 3,
                        MouseEventKind::ScrollDown => app.scroll = app.scroll.saturating_sub(3),
                        _ => {}
                    },
                    _ => {}
                },
                Some(v) = ev_rx.recv() => {
                    if v["type"] == "disconnected" {
                        app.connected = false;
                    } else {
                        app.on_event(v).await;
                    }
                }
                // Animate only while something is happening (no idle CPU).
                _ = tick.tick(), if app.generating => app.spin += 1,
            }
        }
    }
    .await;
    let _ = execute!(std::io::stdout(), DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
    events_task.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_cjk_by_display_width() {
        let lines = wrap(vec![Span::raw("あいうえおかきくけこ")], 6);
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|l| l.iter().map(|s| s.content.width()).sum::<usize>() <= 6));
    }

    #[test]
    fn markdown_lines() {
        let l = markdown("# Title\n- **bold** item\n```rs\nlet x = 1;\n```", 40);
        let text: Vec<String> = l.iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect()).collect();
        assert_eq!(text[0], "Title");
        assert!(text[1].contains("•") && text[1].contains("bold") && !text[1].contains("**"));
        assert!(text.iter().any(|t| t.contains("let x = 1;")));
    }

    fn sample() -> App {
        App {
            client: Client::new("http://127.0.0.1:9", "x"),
            profile: "eve".into(),
            threads: vec![json!({ "id": "t1", "title": "週末の予定", "updated_at": 0 })],
            current: Some("t1".into()),
            messages: vec![
                json!({ "id": "m1", "role": "user", "content": "週末に京都へ行くんだけど、おすすめのプランを教えて！", "status": "complete" }),
                json!({ "id": "m2", "role": "assistant", "status": "complete", "model": "codex-cli/default", "meta": { "provider_ms": 3200 },
                    "content": "いいですね！ **1 泊 2 日** のプランです。\n\n## 1 日目\n- 伏見稲荷大社\n\n```bash\nalice chat\n```" }),
                json!({ "id": "m4", "role": "assistant", "status": "streaming", "content": "予定を登録しますね",
                    "tool_calls": [{ "id": "c1", "name": "calendar_create", "arguments": { "summary": "京都旅行" } }] }),
            ],
            generating: true,
            models: vec!["codex-cli/default".into()],
            default_model: "codex-cli/default".into(),
            pending_model: None,
            input: "雨だったら？".chars().collect(),
            cursor: 6,
            scroll: 0,
            overlay: Overlay::None,
            notice: None,
            spin: 1,
            pending_attachments: vec![],
            quit: false,
            connected: true,
        }
    }

    fn screen(app: &App, w: u16, h: u16) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                let mut line = String::new();
                let mut x = 0;
                while x < buf.area.width {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    x += sym.width().max(1) as u16; // skip the continuation cell of wide chars
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_conversation_and_overlays() {
        let mut app = sample();
        for (w, h) in [(96, 30), (40, 12), (20, 6)] {
            let s = screen(&app, w, h); // must not panic on small terminals
            if w == 96 {
                assert!(s.contains("Alice") && s.contains("週末の予定") && s.contains("eve"), "{s}");
                assert!(s.contains("you") && s.contains("伏見稲荷大社") && s.contains("•"), "{s}");
                assert!(s.contains("calendar_create") && s.contains("雨だったら"), "{s}");
                assert!(!s.contains("**"), "markdown markers leaked: {s}");
            }
        }
        app.overlay = Overlay::Approval(json!({ "approval_id": "a", "tool": "fs_delete", "arguments": { "path": "x" } }));
        assert!(screen(&app, 96, 30).contains("fs_delete"));
        app.overlay = Overlay::Threads(ListState::default().with_selected(Some(0)));
        assert!(screen(&app, 96, 30).contains("スレッド"));
        app.overlay = Overlay::Help;
        assert!(screen(&app, 96, 30).contains("ヘルプ"));
        app.messages.clear();
        app.input.clear();
        app.cursor = 0;
        app.overlay = Overlay::None;
        assert!(screen(&app, 96, 30).contains("なにをお手伝い"));
    }

    #[test]
    fn line_helpers() {
        let b: Vec<char> = "ab\ncd".chars().collect();
        assert_eq!(line_start(&b, 4), 3);
        assert_eq!(line_end(&b, 0), 2);
    }
}
