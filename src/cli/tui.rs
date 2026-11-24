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
    /// `None` while loading.
    Usage(Option<Result<Value, String>>),
}

/// Slash commands: (command, argument hint, description).
const COMMANDS: [(&str, &str, &str); 9] = [
    ("/new", "", "Start a new chat"),
    ("/threads", "", "Browse and switch threads"),
    ("/model", "", "Choose the model for this thread"),
    ("/rename", "<title>", "Rename this thread"),
    ("/delete", "", "Delete this thread and its memories"),
    ("/attach", "<path>", "Attach a file to the next message"),
    ("/usage", "", "Subscription limits and tokens used"),
    ("/help", "", "Keys and commands"),
    ("/exit", "", "Quit"),
];

struct App {
    client: Client,
    profile: String,
    threads: Vec<Value>,
    current: Option<String>,
    messages: Vec<Value>,
    generating: bool,
    models: Vec<String>,
    /// Display names and reasoning efforts (`/api/models` catalog).
    catalog: Vec<Value>,
    /// Effort chosen in the model picker (None = the model's default).
    pick_effort: Option<String>,
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
    /// Selected slash-command suggestion.
    suggest: usize,
    /// Results of background requests, delivered through the event loop.
    bg: Option<mpsc::UnboundedSender<Value>>,
}

impl App {
    /// Commands matching the input while typing a bare `/command`.
    fn suggestions(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        if self.input.first() != Some(&'/') || self.input.iter().any(|c| c.is_whitespace()) {
            return vec![];
        }
        let typed: String = self.input.iter().collect();
        COMMANDS.iter().copied().filter(|(c, _, _)| c.starts_with(typed.as_str())).collect()
    }

    fn open_usage(&mut self) {
        self.overlay = Overlay::Usage(None);
        let (c, tx) = (self.client.clone(), self.bg.clone());
        tokio::spawn(async move {
            let r = c.get("/usage").await;
            if let Some(tx) = tx {
                let _ = tx.send(json!({ "type": "usage.loaded", "result": match r { Ok(v) => json!({ "ok": v }), Err(e) => json!({ "err": e }) } }));
            }
        });
    }

    fn thread(&self) -> Option<&Value> {
        let id = self.current.as_deref()?;
        self.threads.iter().find(|t| t["id"] == id)
    }

    fn info(&self, id: &str) -> Option<&Value> {
        self.catalog.iter().find(|c| c["id"] == id)
    }

    fn efforts(&self, id: &str) -> Vec<String> {
        efforts_in(&self.catalog, id)
    }

    /// "GPT-6.2-Sol · high" for the header.
    fn model_label(&self) -> String {
        let full = self.model();
        let (base, effort) = crate::config::split_effort(&full);
        let name = self.info(base).and_then(|i| i["name"].as_str()).filter(|n| !n.is_empty() && *n != "Default").map(str::to_string).unwrap_or_else(|| base.to_string());
        match effort {
            Some(e) => format!("{name} · {e}"),
            None => name,
        }
    }

    fn open_models(&mut self) {
        let full = self.model();
        let (base, effort) = crate::config::split_effort(&full);
        let sel = self.models.iter().position(|m| m == base).unwrap_or(0);
        self.pick_effort = effort.map(str::to_string);
        self.overlay = Overlay::Models(ListState::default().with_selected(Some(sel)));
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
            self.catalog = v["catalog"].as_array().cloned().unwrap_or_default();
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
            self.note("Still replying — press Esc to stop");
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
            "/model" => self.open_models(),
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
                                    self.note(format!("📎 attached {name}"));
                                }
                                Err(e) => self.error(e),
                            }
                        }
                        Err(e) => self.error(e),
                    },
                    Err(e) => self.error(format!("{arg}: {e}")),
                }
            }
            "/usage" => self.open_usage(),
            "/help" | "/?" => self.overlay = Overlay::Help,
            _ => self.error(format!("Unknown command: {cmd} (try /help)")),
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
                    self.note("This thread was deleted");
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
                    self.note(format!("Approval needed for {} in another thread", v["tool"].as_str().unwrap_or("")));
                }
            }
            "tool.approval_resolved" => {
                if let Overlay::Approval(a) = &self.overlay
                    && a["approval_id"] == v["approval_id"] {
                        self.overlay = Overlay::None;
                    }
            }
            "config.updated" => self.reload_models().await,
            "usage.loaded" => {
                if let Overlay::Usage(slot) = &mut self.overlay {
                    *slot = Some(match v["result"].get("ok") {
                        Some(ok) => Ok(ok.clone()),
                        None => Err(v["result"]["err"].as_str().unwrap_or("failed").to_string()),
                    });
                }
            }
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
            Overlay::Help | Overlay::Usage(_) => {
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
                let highlighted = state.selected().and_then(|i| self.models.get(i)).cloned().unwrap_or_default();
                let efforts = efforts_in(&self.catalog, &highlighted);
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = Overlay::None,
                    KeyCode::Up | KeyCode::Char('k') => state.select(Some(state.selected().unwrap_or(0).saturating_sub(1))),
                    KeyCode::Down | KeyCode::Char('j') => state.select(Some((state.selected().unwrap_or(0) + 1).min(n.saturating_sub(1)))),
                    // ←/→ cycle the reasoning effort: auto, then each supported level.
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') if !efforts.is_empty() => {
                        let cur = self.pick_effort.as_ref().and_then(|e| efforts.iter().position(|x| x == e)).map(|i| i + 1).unwrap_or(0);
                        let len = efforts.len() + 1;
                        let next = if matches!(k.code, KeyCode::Left | KeyCode::Char('h')) { (cur + len - 1) % len } else { (cur + 1) % len };
                        self.pick_effort = (next > 0).then(|| efforts[next - 1].clone());
                    }
                    KeyCode::Enter => {
                        let effort = self.pick_effort.clone().filter(|e| efforts.contains(e));
                        let m = state.selected().and_then(|i| self.models.get(i)).map(|m| match &effort {
                            Some(e) => format!("{m}#{e}"),
                            None => m.clone(),
                        });
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
                            self.note(format!("Model: {m}"));
                        }
                    }
                    _ => {}
                }
                return;
            }
        }
        let sugg = self.suggestions();
        if !sugg.is_empty() {
            let n = sugg.len();
            let sel = self.suggest.min(n - 1);
            match k.code {
                KeyCode::Up => {
                    self.suggest = (sel + n - 1) % n;
                    return;
                }
                KeyCode::Down => {
                    self.suggest = (sel + 1) % n;
                    return;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    let (cmd, arg, _) = sugg[sel];
                    self.suggest = 0;
                    if !arg.is_empty() || k.code == KeyCode::Tab {
                        self.input = format!("{cmd}{}", if arg.is_empty() { "" } else { " " }).chars().collect();
                        self.cursor = self.input.len();
                    } else {
                        self.input.clear();
                        self.cursor = 0;
                        self.command(cmd).await;
                    }
                    return;
                }
                KeyCode::Esc => {
                    self.input.clear();
                    self.cursor = 0;
                    return;
                }
                _ => {}
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
            KeyCode::Char('o') if ctrl => self.open_models(),
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
        self.suggest = 0;
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

fn efforts_in(catalog: &[Value], id: &str) -> Vec<String> {
    let info = catalog.iter().find(|c| c["id"] == id);
    info.and_then(|i| i["efforts"].as_array()).into_iter().flatten().filter_map(|e| e.as_str().map(str::to_string)).collect()
}

fn ends_turn(m: &Value) -> bool {
    m["role"] == "assistant"
        && (m["status"] != "complete" || m["meta"]["final"] == true || m["tool_calls"].as_array().is_none_or(|a| a.is_empty()))
        && m["status"] != "streaming"
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
                    "cancelled" => out.push(Line::from(Span::styled(" (stopped)", s(MUTED)))),
                    "complete" if calls.is_empty() || m["meta"]["final"] == true => {
                        let secs = m.pointer("/meta/provider_ms").and_then(|n| n.as_u64()).unwrap_or(0) as f64 / 1000.0;
                        out.push(Line::from(Span::styled(format!("{} · {secs:.1}s", m.pointer("/meta/model_name").and_then(|n| n.as_str()).or(m["model"].as_str()).unwrap_or("")), s(FAINT))));
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

const GRADIENT: [Color; 5] = [PINK, Color::Rgb(226, 152, 228), LAVENDER, Color::Rgb(150, 190, 240), MINT];

/// Three-line masthead: wordmark + thread title, details, a soft rule.
fn header(f: &mut Frame, app: &App, area: Rect) {
    let mut l1: Vec<Span> = vec![Span::raw("  ")];
    for (i, ch) in "✿ alice".chars().enumerate() {
        let c = if ch == '✿' { PINK } else { GRADIENT[(i - 1).min(4)] };
        l1.push(Span::styled(ch.to_string(), Style::default().fg(c).add_modifier(Modifier::BOLD)));
    }
    let title = app.thread().and_then(|t| t["title"].as_str()).filter(|t| !t.is_empty()).unwrap_or("new chat").to_string();
    l1.push(Span::styled("   ", s(FAINT)));
    l1.push(Span::styled(title, s(TEXT).add_modifier(Modifier::BOLD)));

    let (dot, state) = if app.connected { (MINT, "online") } else { (RED, "offline") };
    let right = vec![Span::styled("● ", s(dot)), Span::styled(state, s(MUTED)), Span::raw("  ")];
    let rw: usize = right.iter().map(|s| s.content.width()).sum();

    let mut l2 = vec![
        Span::raw("    "),
        Span::styled(" ◆ ", s(LAVENDER)),
        Span::styled(app.model_label(), s(LAVENDER)),
        Span::raw(" "),
    ];
    if app.profile != "default" {
        l2.push(Span::styled("  ◆ ", s(PEACH)));
        l2.push(Span::styled(format!("profile {}", app.profile), s(PEACH)));
    }
    if !app.pending_attachments.is_empty() {
        l2.push(Span::styled(format!("  📎 {}", app.pending_attachments.len()), s(MINT)));
    }

    let w = area.width.saturating_sub(4) as usize;
    let rule: Vec<Span> = (0..w)
        .map(|i| Span::styled("─", s(GRADIENT[i * GRADIENT.len() / w.max(1)]).add_modifier(Modifier::DIM)))
        .collect();
    let mut l3 = vec![Span::raw("  ")];
    l3.extend(rule);

    f.render_widget(Paragraph::new(vec![Line::from(l1), Line::from(l2), Line::from(l3)]), area);
    if area.width as usize > rw + 20 {
        let r = Rect { x: area.x + area.width - rw as u16, width: rw as u16, height: 1, ..area };
        f.render_widget(Paragraph::new(Line::from(right)), r);
    }
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let input_text: String = app.input.iter().collect();
    let input_width = area.width.saturating_sub(6) as usize;
    let input_lines: usize = input_text.split('\n').map(|l| (l.width() / input_width.max(1)) + 1).sum::<usize>().clamp(1, 8);
    let head_h = if area.height >= 12 { 3 } else { 1 };
    let [head, body, input] =
        Layout::vertical([Constraint::Length(head_h), Constraint::Min(3), Constraint::Length(input_lines as u16 + 2)]).areas(area);
    header(f, app, head);

    // Conversation
    let inner = Rect { x: body.x + 2, y: body.y + 1.min(body.height), width: body.width.saturating_sub(4), height: body.height.saturating_sub(1) };
    if app.messages.is_empty() {
        let h = inner.height / 2;
        let lines = vec![
            Line::from(Span::styled("✿", s(PINK).add_modifier(Modifier::BOLD))).alignment(Alignment::Center),
            Line::from(Span::styled("What can I help with?", s(TEXT).add_modifier(Modifier::BOLD))).alignment(Alignment::Center),
            Line::from(Span::styled("type / for commands", s(FAINT))).alignment(Alignment::Center),
        ];
        f.render_widget(Paragraph::new(lines), Rect { y: inner.y + h.saturating_sub(2), height: 3.min(inner.height), ..inner });
    } else {
        let lines = conversation(app, inner.width as usize);
        let total = lines.len();
        let view = inner.height as usize;
        let max_scroll = total.saturating_sub(view);
        let scroll = app.scroll.min(max_scroll);
        let start = total.saturating_sub(view + scroll);
        let visible: Vec<Line> = lines.into_iter().skip(start).take(view).collect();
        f.render_widget(Paragraph::new(visible), inner);
        if scroll > 0 && inner.height > 0 {
            let tag = Span::styled(format!(" ↓ {scroll} "), Style::default().fg(Color::Black).bg(LAVENDER));
            f.render_widget(Paragraph::new(Line::from(tag)).alignment(Alignment::Right), Rect { y: inner.y + inner.height - 1, height: 1, ..inner });
        }
    }

    // Input
    let border = if app.generating { LAVENDER } else { PINK };
    let mut block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(s(border));
    if app.generating {
        block = block.title(Span::styled(format!(" {} thinking · esc to stop ", SPINNER[app.spin % 4]), s(LAVENDER)));
    }
    match &app.notice {
        Some((msg, true)) => block = block.title_bottom(Span::styled(format!(" ⚠ {msg} "), s(RED))),
        Some((msg, false)) => block = block.title_bottom(Span::styled(format!(" ✿ {msg} "), s(MINT))),
        None => {}
    }
    let in_inner = block.inner(input);
    f.render_widget(block, input);
    let mut ilines: Vec<Line> = Vec::new();
    let mut cursor_pos = (0u16, 0u16);
    let width = in_inner.width.saturating_sub(2).max(1) as usize;
    // Lay out input with soft wraps, tracking the cursor.
    let mut row = 0usize;
    let mut col = 0usize;
    let mut cur_line: Vec<Span> = vec![Span::styled("› ", s(PINK))];
    for (idx, ch) in input_text.chars().enumerate() {
        if idx == app.cursor {
            cursor_pos = (col as u16, row as u16);
        }
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
    if app.cursor >= app.input.len() {
        cursor_pos = (col as u16, row as u16);
    }
    ilines.push(Line::from(cur_line));
    if app.input.is_empty() {
        ilines = vec![Line::from(vec![Span::styled("› ", s(PINK)), Span::styled("Message Alice…", s(FAINT))])];
    }
    let skip = (cursor_pos.1 as usize + 1).saturating_sub(in_inner.height as usize);
    f.render_widget(Paragraph::new(ilines.into_iter().skip(skip).collect::<Vec<_>>()), in_inner);
    if matches!(app.overlay, Overlay::None) && in_inner.height > 0 {
        f.set_cursor_position(Position::new(in_inner.x + 2 + cursor_pos.0, in_inner.y + cursor_pos.1 - skip as u16));
    }

    // Slash-command suggestions, floating just above the input.
    let sugg = app.suggestions();
    if !sugg.is_empty() && matches!(app.overlay, Overlay::None) {
        let h = (sugg.len() as u16 + 2).min(input.y.saturating_sub(area.y));
        let w = 58.min(area.width.saturating_sub(4));
        let r = Rect { x: input.x + 2, y: input.y.saturating_sub(h), width: w, height: h };
        let sel = app.suggest.min(sugg.len() - 1);
        let items: Vec<ListItem> = sugg
            .iter()
            .map(|(c, a, d)| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{c:<9}"), s(PINK).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{a:<8} "), s(FAINT)),
                    Span::styled(d.to_string(), s(TEXT)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(s(FAINT)).title_bottom(Span::styled(" ↑↓ select · tab complete · ⏎ run ", s(FAINT))))
            .highlight_style(Style::default().bg(BUBBLE))
            .highlight_symbol("✿ ");
        f.render_widget(Clear, r);
        f.render_stateful_widget(list, r, &mut ListState::default().with_selected(Some(sel)));
    }

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
                    let title = t["title"].as_str().filter(|s| !s.is_empty()).unwrap_or("(untitled)");
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
                .block(popup("Threads  ⏎ open · n new · d delete · esc"))
                .highlight_style(Style::default().bg(BUBBLE).add_modifier(Modifier::BOLD))
                .highlight_symbol("✿ ");
            let mut st = *state;
            f.render_stateful_widget(list, r, &mut st);
        }
        Overlay::Models(state) => {
            let r = centered(area, 72, (app.models.len() as u16 + 2).clamp(4, 20));
            f.render_widget(Clear, r);
            let full = app.model();
            let cur = crate::config::split_effort(&full).0;
            let default = crate::config::split_effort(&app.default_model).0;
            let sel = state.selected().unwrap_or(0);
            let items: Vec<ListItem> = app
                .models
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let info = app.info(m);
                    let name = info.and_then(|x| x["name"].as_str()).filter(|n| !n.is_empty()).unwrap_or(m);
                    let mut spans = vec![Span::styled(name.to_string(), s(TEXT))];
                    if name != m {
                        spans.push(Span::styled(format!("  {m}"), s(FAINT)));
                    }
                    if m == cur {
                        spans.push(Span::styled("  ✓", s(MINT)));
                    }
                    if m == default {
                        spans.push(Span::styled("  default", s(FAINT)));
                    }
                    let efforts = app.efforts(m);
                    if i == sel && !efforts.is_empty() {
                        let dflt = info.and_then(|x| x["default_effort"].as_str()).unwrap_or("-");
                        let e = app.pick_effort.clone().filter(|e| efforts.contains(e)).unwrap_or_else(|| format!("auto ({dflt})"));
                        spans.push(Span::styled(format!("   ‹ {e} ›"), s(PINK).add_modifier(Modifier::BOLD)));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let list = List::new(items).block(popup("Model  ⏎ select · ←→ reasoning · esc")).highlight_style(Style::default().bg(BUBBLE)).highlight_symbol("✿ ");
            let mut st = *state;
            f.render_stateful_widget(list, r, &mut st);
        }
        Overlay::Approval(a) => {
            let r = centered(area, 70, 12);
            f.render_widget(Clear, r);
            let args = serde_json::to_string_pretty(&a["arguments"]).unwrap_or_default();
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("🔐 Allow ", s(TEXT)),
                    Span::styled(a["tool"].as_str().unwrap_or("").to_string(), s(PEACH).add_modifier(Modifier::BOLD)),
                    Span::styled(" to run?", s(TEXT)),
                ]),
                Line::default(),
            ];
            for l in args.lines().take(6) {
                lines.push(Line::from(Span::styled(l.to_string(), Style::default().fg(MINT).bg(CODE_BG))));
            }
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::styled(" y ", Style::default().fg(Color::Black).bg(MINT)),
                Span::styled(" allow   ", s(TEXT)),
                Span::styled(" n ", Style::default().fg(Color::Black).bg(RED)),
                Span::styled(" deny", s(TEXT)),
            ]));
            f.render_widget(Paragraph::new(lines).block(popup("Approval")), r);
        }
        Overlay::ConfirmDelete => {
            let r = centered(area, 54, 6);
            f.render_widget(Clear, r);
            let lines = vec![
                Line::from(Span::styled("Delete this thread?", s(TEXT).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled("Memories learned from it are deleted too.", s(MUTED))),
                Line::from(vec![
                    Span::styled(" y ", Style::default().fg(Color::Black).bg(RED)),
                    Span::styled(" delete   ", s(TEXT)),
                    Span::styled(" esc ", Style::default().fg(Color::Black).bg(MUTED)),
                    Span::styled(" cancel", s(TEXT)),
                ]),
            ];
            f.render_widget(Paragraph::new(lines).block(popup("Delete")), r);
        }
        Overlay::Rename(buf) => {
            let r = centered(area, 60, 3);
            f.render_widget(Clear, r);
            f.render_widget(Paragraph::new(Line::from(Span::styled(buf.clone(), s(TEXT)))).block(popup("Rename  ⏎ save · esc")), r);
            f.set_cursor_position(Position::new(r.x + 1 + buf.width() as u16, r.y + 1));
        }
        Overlay::Usage(state) => {
            let lines = usage_lines(state, app.spin);
            let r = centered(area, 74, lines.len() as u16 + 2);
            f.render_widget(Clear, r);
            f.render_widget(Paragraph::new(lines).block(popup("Usage")), r);
        }
        Overlay::Help => {
            let r = centered(area, 62, 22);
            f.render_widget(Clear, r);
            let row = |k: &str, d: &str| Line::from(vec![Span::styled(format!("  {k:<16}"), s(PINK)), Span::styled(d.to_string(), s(TEXT))]);
            let mut lines = vec![
                row("enter", "send"),
                row("alt+enter / ^J", "new line"),
                row("esc", "stop the reply"),
                row("ctrl+t", "threads"),
                row("ctrl+n", "new chat"),
                row("ctrl+o", "model"),
                row("pgup / pgdn", "scroll (or mouse wheel)"),
                row("ctrl+c", "stop / quit"),
                Line::default(),
            ];
            for (c, a, d) in COMMANDS {
                lines.push(row(&format!("{c} {a}"), d));
            }
            lines.push(Line::from(Span::styled("  press any key to close", s(FAINT))));
            f.render_widget(Paragraph::new(lines).block(popup("Help")), r);
        }
    }
}

fn usage_lines(state: &Option<Result<Value, String>>, spin: usize) -> Vec<Line<'static>> {
    let v = match state {
        None => return vec![Line::from(Span::styled(format!(" {} checking…", SPINNER[spin % 4]), s(MUTED)))],
        Some(Err(e)) => return vec![Line::from(Span::styled(format!(" ⚠ {e}"), s(RED)))],
        Some(Ok(v)) => v,
    };
    let mut out = Vec::new();
    let subs = v["subscriptions"].as_array().cloned().unwrap_or_default();
    if subs.is_empty() {
        out.push(Line::from(Span::styled(" No subscription (codex-cli) providers.", s(MUTED))));
    }
    for sub in &subs {
        let plan = sub.pointer("/limits/plan").and_then(|p| p.as_str()).unwrap_or("").to_string();
        out.push(Line::from(vec![
            Span::styled(format!(" {}", sub["id"].as_str().unwrap_or("")), s(LAVENDER).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {plan}"), s(FAINT)),
        ]));
        if sub["ok"] != true {
            out.push(Line::from(Span::styled(format!("   {}", sub["error"].as_str().unwrap_or("")), s(RED))));
            continue;
        }
        for (name, used, reset) in crate::usage::windows(&sub["limits"]) {
            let col = if used >= 90.0 { RED } else if used >= 70.0 { PEACH } else { MINT };
            out.push(Line::from(vec![
                Span::styled(format!("   {name:<7}"), s(TEXT)),
                Span::styled(crate::usage::bar(used, 20), s(col)),
                Span::styled(format!(" {:>3.0}% left", 100.0 - used), s(TEXT).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  resets {reset}"), s(MUTED)),
            ]));
        }
    }
    for (label, key) in [("last 24h", "day"), ("last 30 days", "month")] {
        out.push(Line::default());
        out.push(Line::from(Span::styled(format!(" tokens · {label}"), s(LAVENDER).add_modifier(Modifier::BOLD))));
        let rows = v["tokens"][key].as_array().cloned().unwrap_or_default();
        if rows.is_empty() {
            out.push(Line::from(Span::styled("   —", s(FAINT))));
        }
        for r in rows.iter().take(5) {
            out.push(Line::from(vec![
                Span::styled(format!("   {:<26}", crate::util::truncate(r["model"].as_str().unwrap_or(""), 26)), s(TEXT)),
                Span::styled(format!("in {:>7}", crate::usage::compact(r["input_tokens"].as_i64().unwrap_or(0))), s(MUTED)),
                Span::styled(format!("  out {:>7}", crate::usage::compact(r["output_tokens"].as_i64().unwrap_or(0))), s(MUTED)),
                Span::styled(format!("  {} replies", r["responses"]), s(FAINT)),
            ]));
        }
    }
    out
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
        catalog: Vec::new(),
        pick_effort: None,
        input: vec![],
        cursor: 0,
        scroll: 0,
        overlay: Overlay::None,
        notice: None,
        spin: 0,
        pending_attachments: vec![],
        quit: false,
        connected: false,
        suggest: 0,
        bg: None,
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
    app.bg = Some(ev_tx.clone());
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
                _ = tick.tick(), if app.generating || matches!(app.overlay, Overlay::Usage(None)) => app.spin += 1,
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
        catalog: Vec::new(),
        pick_effort: None,
            input: "雨だったら？".chars().collect(),
            cursor: 6,
            scroll: 0,
            overlay: Overlay::None,
            notice: None,
            spin: 1,
            pending_attachments: vec![],
            quit: false,
            connected: true,
            suggest: 0,
            bg: None,
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
                assert!(s.contains("alice") && s.contains("週末の予定") && s.contains("profile eve") && s.contains("online"), "{s}");
                assert!(s.contains("you") && s.contains("伏見稲荷大社") && s.contains("•"), "{s}");
                assert!(s.contains("calendar_create") && s.contains("雨だったら"), "{s}");
                assert!(!s.contains("**"), "markdown markers leaked: {s}");
            }
        }
        app.overlay = Overlay::Approval(json!({ "approval_id": "a", "tool": "fs_delete", "arguments": { "path": "x" } }));
        assert!(screen(&app, 96, 30).contains("fs_delete"));
        app.overlay = Overlay::Threads(ListState::default().with_selected(Some(0)));
        assert!(screen(&app, 96, 30).contains("Threads"));
        app.overlay = Overlay::Help;
        assert!(screen(&app, 96, 30).contains("/usage"));
        app.overlay = Overlay::Usage(Some(Ok(json!({
            "subscriptions": [{ "id": "codex-cli", "ok": true, "limits": { "plan": "plus",
                "primary": { "used_percent": 30, "window_minutes": 300, "resets_at": crate::util::now_ms() + 3_600_000 } } }],
            "tokens": { "day": [{ "model": "codex-cli/default", "input_tokens": 1200, "output_tokens": 300, "responses": 2 }], "month": [] } }))));
        let u = screen(&app, 96, 30);
        assert!(u.contains("70% left") && u.contains("1.2k"), "{u}");
        app.messages.clear();
        app.input = vec!['/', 'r'];
        app.cursor = 2;
        app.overlay = Overlay::None;
        let sg = screen(&app, 96, 30);
        assert!(sg.contains("/rename") && !sg.contains("/usage"), "{sg}");
        app.input.clear();
        app.cursor = 0;
        assert!(screen(&app, 96, 30).contains("What can I help with?"));
    }

    #[test]
    fn line_helpers() {
        let b: Vec<char> = "ab\ncd".chars().collect();
        assert_eq!(line_start(&b, 4), 3);
        assert_eq!(line_end(&b, 0), 2);
    }
}
