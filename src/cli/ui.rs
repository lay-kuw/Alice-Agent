//! The look of the interactive CLI: a soft pastel rail down the left
//! (┌ │ ◆ ◇ └), arrow-key menus, toggles, masked input and spinners.
//!
//! ```text
//! ┌  ✦ Alice · setup
//! │
//! ◇  Port
//! │  7878
//! │
//! ◆  Provider
//! │  ● codex-cli        ChatGPT subscription
//! │  ○ antigravity-cli  Google AI subscription
//! └
//! ```
//!
//! Without a terminal (pipes, scripts, NO_COLOR) the same questions are asked
//! one plain line at a time, so setup can still be scripted.

use super::chat::color;
use ratatui::crossterm::cursor::{Hide, MoveToColumn, MoveUp, Show};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::terminal::{self, Clear, ClearType};
use ratatui::crossterm::{execute, queue};
use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use unicode_width::UnicodeWidthStr;

// ---------------------------------------------------------------- colors

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const STRIKE: &str = "\x1b[9m";
const LAVENDER: &str = "\x1b[38;2;184;166;245m";
const PINK: &str = "\x1b[38;2;247;158;199m";
const MINT: &str = "\x1b[38;2;140;219;191m";
const PEACH: &str = "\x1b[38;2;255;178;138m";
const ROSE: &str = "\x1b[38;2;240;120;133m";
const SKY: &str = "\x1b[38;2;145;204;250m";
const LEMON: &str = "\x1b[38;2;250;219;122m";
const INK: &str = "\x1b[38;2;40;30;70m";
const BG_PINK: &str = "\x1b[48;2;247;158;199m";

fn paint(codes: &[&str], s: &str) -> String {
    if color() { format!("{}{s}{RESET}", codes.concat()) } else { s.to_string() }
}

pub fn lavender(s: &str) -> String {
    paint(&[LAVENDER], s)
}
pub fn pink(s: &str) -> String {
    paint(&[PINK], s)
}
pub fn mint(s: &str) -> String {
    paint(&[MINT], s)
}
pub fn peach(s: &str) -> String {
    paint(&[PEACH], s)
}
pub fn rose(s: &str) -> String {
    paint(&[ROSE], s)
}
pub fn sky(s: &str) -> String {
    paint(&[SKY], s)
}
pub fn lemon(s: &str) -> String {
    paint(&[LEMON], s)
}
pub fn bold(s: &str) -> String {
    paint(&[BOLD], s)
}
pub fn dim(s: &str) -> String {
    paint(&[DIM], s)
}
pub fn strike(s: &str) -> String {
    paint(&[DIM, STRIKE], s)
}

/// A filled pill: ` ✦ setup `.
pub fn pill(s: &str) -> String {
    if color() { format!("{BG_PINK}{INK}{BOLD} {s} {RESET}") } else { format!("[{s}]") }
}

/// A quieter pill for tags such as `ready` or `ask`.
pub fn tag(s: &str, tone: Tone) -> String {
    if !color() {
        return format!("[{s}]");
    }
    let fg = match tone {
        Tone::Good => MINT,
        Tone::Warn => PEACH,
        Tone::Bad => ROSE,
        Tone::Info => SKY,
        Tone::Soft => LAVENDER,
        Tone::Off => "\x1b[38;2;150;145;170m",
    };
    format!("{fg}⟨{s}⟩{RESET}")
}

#[derive(Clone, Copy)]
pub enum Tone {
    Good,
    Warn,
    Bad,
    Info,
    Soft,
    Off,
}

/// Interactive when both ends are a terminal.
pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb")
}

// ---------------------------------------------------------------- the rail

fn bar() -> String {
    lavender("│")
}

/// True right after an empty rail line, so two never stack up.
static GAPPED: AtomicBool = AtomicBool::new(false);

fn spacer() {
    if !GAPPED.swap(true, Ordering::Relaxed) {
        println!("{}", bar());
    }
}

fn mark() {
    GAPPED.store(false, Ordering::Relaxed);
}

/// The rail line a live prompt starts with (none right after an empty one).
fn lead() -> Vec<String> {
    if GAPPED.swap(false, Ordering::Relaxed) { vec![] } else { vec![bar()] }
}

/// Shorten to `max` columns with an ellipsis.
pub fn clip(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if out.width() + 1 >= max {
            break;
        }
        out.push(c);
    }
    format!("{}…", out.trim_end())
}

/// Pad a tag (or any colored text) to `width` visible columns.
pub fn pad(s: &str, width: usize) -> String {
    let w = strip(s).width();
    format!("{s}{}", " ".repeat(width.saturating_sub(w)))
}

/// The first line of a flow: `┌  ✦ Alice · setup`.
pub fn intro(title: &str) {
    println!();
    println!("{}  {}", lavender("┌"), pill(&format!("✦ {title}")));
    mark();
    spacer();
}

/// The last line of a flow: `└  All set ♡`.
pub fn outro(text: &str) {
    spacer();
    mark();
    println!("{}  {text}", lavender("└"));
    println!();
}

/// A titled step on the rail: `◆  Server  2/5 ●●○○○`.
pub fn step(n: usize, total: usize, title: &str) {
    let dots: String = (1..=total).map(|i| if i <= n { "●" } else { "○" }).collect();
    spacer();
    println!("{}  {}  {}", pink("◆"), bold(title), dim(&format!("{n}/{total} {dots}")));
    mark();
}

/// A section on the rail without a number.
pub fn heading(title: &str) {
    spacer();
    println!("{}  {}", pink("◆"), bold(title));
    mark();
}

/// An empty rail line.
pub fn gap() {
    spacer();
}

/// Dim lines on the rail.
pub fn note(text: &str) {
    for l in text.lines() {
        println!("{}  {}", bar(), dim(l));
    }
    mark();
}

/// A plain line on the rail.
pub fn line(text: &str) {
    println!("{}  {text}", bar());
    mark();
}

pub fn ok(text: &str) {
    println!("{}  {} {text}", bar(), mint("✓"));
    mark();
}
pub fn warn(text: &str) {
    println!("{}  {} {text}", bar(), peach("!"));
    mark();
}
pub fn fail(text: &str) {
    println!("{}  {} {text}", bar(), rose("✗"));
    mark();
}

/// Aligned `label   value` rows on the rail.
pub fn kv(rows: &[(&str, String)]) {
    let w = rows.iter().map(|(k, _)| k.width()).max().unwrap_or(0);
    for (k, v) in rows {
        println!("{}  {}{}  {v}", bar(), dim(k), " ".repeat(w - k.width()));
    }
    mark();
}

/// A path with the home folder as `~`.
pub fn path(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

/// An error at the top level (after a flow or on its own).
pub fn error(text: &str) {
    eprintln!("{}  {}", rose("✗"), text);
}

// ---------------------------------------------------------------- raw-mode drawing

/// Restores the terminal however the prompt ends.
struct Raw;

impl Raw {
    fn on() -> Option<Raw> {
        terminal::enable_raw_mode().ok()?;
        let _ = execute!(std::io::stdout(), Hide);
        Some(Raw)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), Show);
        let _ = terminal::disable_raw_mode();
    }
}

/// Lines drawn by the live prompt, so it can be redrawn in place.
struct Frame {
    rows: u16,
    /// The spacer line the prompt starts with (or none).
    lead: Vec<String>,
}

impl Frame {
    fn new() -> Frame {
        Frame { rows: 0, lead: lead() }
    }

    fn draw(&mut self, lines: &[String]) {
        let mut out = std::io::stdout();
        if self.rows > 0 {
            let _ = queue!(out, MoveUp(self.rows), MoveToColumn(0));
        }
        let _ = queue!(out, MoveToColumn(0), Clear(ClearType::FromCursorDown));
        let cols = terminal::size().map(|(c, _)| c.max(20) as usize).unwrap_or(80);
        let mut rows = 0u16;
        for l in lines {
            let _ = write!(out, "{l}\r\n");
            let w = strip(l).width().max(1);
            rows += w.div_ceil(cols) as u16;
        }
        let _ = out.flush();
        self.rows = rows;
    }
}

/// Text without escape codes (for measuring).
fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut esc = false;
    for c in s.chars() {
        match (esc, c) {
            (false, '\x1b') => esc = true,
            (true, 'm') => esc = false,
            (true, _) => {}
            (false, c) => out.push(c),
        }
    }
    out
}

fn key() -> Option<KeyEvent> {
    loop {
        match event::read().ok()? {
            Event::Key(k) if k.kind != KeyEventKind::Release => return Some(k),
            _ => {}
        }
    }
}

fn cancelled(k: &KeyEvent) -> bool {
    k.code == KeyCode::Esc || (k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c' | 'd')))
}

/// Ctrl+C / Esc: leave the flow the way a cancelled prompt does.
fn bail(frame: &mut Frame, prompt: &str) -> ! {
    let mut lines = frame.lead.clone();
    lines.push(format!("{}  {}", rose("■"), strike(prompt)));
    lines.push(format!("{}  {}", lavender("└"), dim("cancelled")));
    frame.draw(&lines);
    let _ = execute!(std::io::stdout(), Show);
    let _ = terminal::disable_raw_mode();
    println!();
    std::process::exit(130);
}

/// The answered prompt, folded: `◇  Port` / `│  7878`.
fn answered(frame: &mut Frame, prompt: &str, answer: &str) {
    let mut lines = frame.lead.clone();
    lines.push(format!("{}  {}", lavender("◇"), prompt));
    lines.push(format!("{}  {}", bar(), dim(answer)));
    frame.draw(&lines);
    mark();
}

// ---------------------------------------------------------------- prompts

/// A line of text. Empty input keeps `default`.
pub fn text(prompt: &str, default: &str) -> String {
    input(prompt, default, false)
}

/// Hidden input, shown as dots.
pub fn password(prompt: &str) -> String {
    input(prompt, "", true)
}

fn input(prompt: &str, default: &str, secret: bool) -> String {
    let prompt = prompt.trim();
    if !interactive() {
        return plain_line(prompt, default, secret);
    }
    let Some(_raw) = Raw::on() else { return plain_line(prompt, default, secret) };
    let mut frame = Frame::new();
    let mut buf = String::new();
    loop {
        let shown = if secret {
            "•".repeat(buf.chars().count())
        } else {
            buf.clone()
        };
        let field = if buf.is_empty() && !default.is_empty() {
            format!("{}{}", pink("▏"), dim(default))
        } else {
            format!("{shown}{}", pink("▏"))
        };
        let mut lines = frame.lead.clone();
        lines.extend([
            format!("{}  {}", pink("◆"), bold(prompt)),
            format!("{}  {field}", pink("│")),
            format!("{}  {}", pink("└"), dim(if secret { "hidden · Enter to confirm · Esc to cancel" } else { "Enter to confirm · Esc to cancel" })),
        ]);
        frame.draw(&lines);
        let Some(k) = key() else { break };
        if cancelled(&k) {
            bail(&mut frame, prompt);
        }
        match k.code {
            KeyCode::Enter => break,
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => buf.clear(),
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => buf.push(c),
            KeyCode::Tab if buf.is_empty() => buf = default.to_string(),
            _ => {}
        }
    }
    let value = if buf.trim().is_empty() { default.to_string() } else { buf.trim().to_string() };
    let shown = if secret {
        if value.is_empty() { "(empty)".into() } else { "•".repeat(value.chars().count().min(12)) }
    } else if value.is_empty() {
        "(empty)".into()
    } else {
        value.clone()
    };
    answered(&mut frame, prompt, &shown);
    value
}

/// Pick one of `items` (label, hint) with ↑↓ / j k / numbers and Enter.
pub fn select(prompt: &str, items: &[(&str, &str)], default: usize) -> usize {
    let prompt = prompt.trim();
    if items.is_empty() {
        return 0;
    }
    if !interactive() {
        return plain_select(prompt, items, default);
    }
    let Some(_raw) = Raw::on() else { return plain_select(prompt, items, default) };
    let mut frame = Frame::new();
    let mut at = default.min(items.len() - 1);
    let w = items.iter().map(|(l, _)| l.width()).max().unwrap_or(0);
    // Long lists scroll in a window around the cursor.
    let window = terminal::size().map(|(_, r)| (r as usize).saturating_sub(6).clamp(5, 14)).unwrap_or(10);
    loop {
        let start = at.saturating_sub(window / 2).min(items.len().saturating_sub(window));
        let mut lines = frame.lead.clone();
        lines.push(format!("{}  {}", pink("◆"), bold(prompt)));
        if start > 0 {
            lines.push(format!("{}  {}", pink("│"), dim("  ↑ more")));
        }
        for (i, (label, hint)) in items.iter().enumerate().skip(start).take(window) {
            let pad = " ".repeat(w - label.width());
            let hint = if hint.is_empty() { String::new() } else { format!("  {}", dim(hint)) };
            if i == at {
                lines.push(format!("{}  {} {}{pad}{hint}", pink("│"), pink("●"), bold(label)));
            } else {
                lines.push(format!("{}  {} {}{pad}{hint}", pink("│"), dim("○"), label));
            }
        }
        if start + window < items.len() {
            lines.push(format!("{}  {}", pink("│"), dim("  ↓ more")));
        }
        lines.push(format!("{}  {}", pink("└"), dim("↑↓ move · Enter choose · Esc cancel")));
        frame.draw(&lines);
        let Some(k) = key() else { break };
        if cancelled(&k) {
            bail(&mut frame, prompt);
        }
        match k.code {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => at = if at == 0 { items.len() - 1 } else { at - 1 },
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => at = (at + 1) % items.len(),
            KeyCode::Home => at = 0,
            KeyCode::End => at = items.len() - 1,
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let n = c.to_digit(10).unwrap_or(0) as usize;
                if (1..=items.len()).contains(&n) {
                    at = n - 1;
                }
            }
            KeyCode::Enter => break,
            _ => {}
        }
    }
    answered(&mut frame, prompt, items[at].0);
    at
}

/// Pick any number of `items` with Space; Enter confirms.
pub fn multiselect(prompt: &str, items: &[(&str, &str)], on: &[bool]) -> Vec<bool> {
    let prompt = prompt.trim();
    let mut picked: Vec<bool> = (0..items.len()).map(|i| on.get(i).copied().unwrap_or(false)).collect();
    if items.is_empty() {
        return picked;
    }
    if !interactive() {
        return plain_multi(prompt, items, &picked);
    }
    let Some(_raw) = Raw::on() else { return plain_multi(prompt, items, &picked) };
    let mut frame = Frame::new();
    let mut at = 0;
    let w = items.iter().map(|(l, _)| l.width()).max().unwrap_or(0);
    loop {
        let mut lines = frame.lead.clone();
        lines.push(format!("{}  {}", pink("◆"), bold(prompt)));
        for (i, (label, hint)) in items.iter().enumerate() {
            let pad = " ".repeat(w - label.width());
            let hint = if hint.is_empty() { String::new() } else { format!("  {}", dim(hint)) };
            let box_ = if picked[i] { mint("◼") } else { dim("◻") };
            let label = if i == at { bold(label) } else { label.to_string() };
            let cursor = if i == at { pink("›") } else { " ".into() };
            lines.push(format!("{}  {cursor}{box_} {label}{pad}{hint}", pink("│")));
        }
        lines.push(format!("{}  {}", pink("└"), dim("↑↓ move · Space toggle · a all · Enter done")));
        frame.draw(&lines);
        let Some(k) = key() else { break };
        if cancelled(&k) {
            bail(&mut frame, prompt);
        }
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => at = if at == 0 { items.len() - 1 } else { at - 1 },
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => at = (at + 1) % items.len(),
            KeyCode::Char(' ') => picked[at] = !picked[at],
            KeyCode::Char('a') => {
                let all = picked.iter().all(|p| *p);
                picked.iter_mut().for_each(|p| *p = !all);
            }
            KeyCode::Enter => break,
            _ => {}
        }
    }
    let chosen: Vec<&str> = items.iter().zip(&picked).filter(|(_, p)| **p).map(|((l, _), _)| *l).collect();
    answered(&mut frame, prompt, &if chosen.is_empty() { "(none)".to_string() } else { chosen.join(", ") });
    picked
}

/// Yes or no, as a toggle (← → / y n / Enter).
pub fn confirm(prompt: &str, default: bool) -> bool {
    let prompt = prompt.trim();
    if !interactive() {
        let a = plain_line(&format!("{prompt} ({})", if default { "Y/n" } else { "y/N" }), "", false);
        return match a.to_lowercase().as_str() {
            "" => default,
            "y" | "yes" => true,
            _ => false,
        };
    }
    let Some(_raw) = Raw::on() else { return default };
    let mut frame = Frame::new();
    let mut yes = default;
    loop {
        let (y, n) = if yes { (format!("{} {}", pink("●"), bold("Yes")), format!("{} No", dim("○"))) } else { (format!("{} Yes", dim("○")), format!("{} {}", pink("●"), bold("No"))) };
        let mut lines = frame.lead.clone();
        lines.extend([
            format!("{}  {}", pink("◆"), bold(prompt)),
            format!("{}  {y}  {}  {n}", pink("│"), dim("/")),
            format!("{}  {}", pink("└"), dim("← → or y / n · Enter confirm")),
        ]);
        frame.draw(&lines);
        let Some(k) = key() else { break };
        if cancelled(&k) {
            bail(&mut frame, prompt);
        }
        match k.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('h' | 'l') => yes = !yes,
            KeyCode::Char('y' | 'Y') => {
                yes = true;
                break;
            }
            KeyCode::Char('n' | 'N') => {
                yes = false;
                break;
            }
            KeyCode::Enter => break,
            _ => {}
        }
    }
    answered(&mut frame, prompt, if yes { "Yes" } else { "No" });
    yes
}

// ---------------------------------------------------------------- plain fallbacks

fn plain_line(prompt: &str, default: &str, secret: bool) -> String {
    if default.is_empty() {
        print!("{prompt}: ");
    } else {
        print!("{prompt} [{default}]: ");
    }
    let _ = std::io::stdout().flush();
    let tty = secret
        && std::io::stdin().is_terminal()
        && std::process::Command::new("stty").arg("-echo").stdin(std::process::Stdio::inherit()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    if tty {
        let _ = std::process::Command::new("stty").arg("echo").stdin(std::process::Stdio::inherit()).status();
        println!();
    }
    let s = s.trim();
    if s.is_empty() { default.to_string() } else { s.to_string() }
}

fn plain_select(prompt: &str, items: &[(&str, &str)], default: usize) -> usize {
    for (i, (l, h)) in items.iter().enumerate() {
        println!("  {}) {l}{}", i + 1, if h.is_empty() { String::new() } else { format!("  {h}") });
    }
    loop {
        let a = plain_line(prompt, &(default + 1).to_string(), false);
        if let Ok(n) = a.parse::<usize>()
            && (1..=items.len()).contains(&n) {
                return n - 1;
            }
    }
}

fn plain_multi(prompt: &str, items: &[(&str, &str)], on: &[bool]) -> Vec<bool> {
    let ids: Vec<&str> = items.iter().map(|(l, _)| *l).collect();
    let cur: Vec<&str> = ids.iter().zip(on).filter(|(_, p)| **p).map(|(l, _)| *l).collect();
    let a = plain_line(&format!("{prompt} ({})", ids.join(",")), &cur.join(","), false);
    let chosen: Vec<&str> = a.split(',').map(str::trim).collect();
    ids.iter().map(|l| chosen.contains(l)).collect()
}

// ---------------------------------------------------------------- spinner

/// A little sparkle that spins on the rail while something runs.
pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    text: String,
}

pub fn spinner(text: &str) -> Spinner {
    let stop = Arc::new(AtomicBool::new(false));
    let handle = (interactive() && color()).then(|| {
        let stop = stop.clone();
        let text = text.to_string();
        std::thread::spawn(move || {
            const FRAMES: [&str; 6] = ["✦", "✧", "⋆", "✧", "✦", "⋆"];
            const TONES: [&str; 3] = [PINK, LAVENDER, SKY];
            let mut i = 0usize;
            let mut out = std::io::stdout();
            let _ = execute!(out, Hide);
            while !stop.load(Ordering::Relaxed) {
                let f = format!("{}{}{RESET}", TONES[i % 3], FRAMES[i % FRAMES.len()]);
                let _ = write!(out, "\r{}  {f} {}", bar(), dim(&text));
                let _ = out.flush();
                i += 1;
                std::thread::sleep(std::time::Duration::from_millis(110));
            }
            let _ = execute!(out, MoveToColumn(0), Clear(ClearType::CurrentLine), Show);
        })
    });
    if handle.is_none() {
        note(&format!("{text} …"));
    }
    Spinner { stop, handle, text: text.to_string() }
}

impl Spinner {
    fn end(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
    pub fn done(mut self, text: &str) {
        self.end();
        ok(text);
    }
    pub fn failed(mut self, text: &str) {
        self.end();
        fail(text);
    }
    /// Stop without a line (the caller prints its own).
    pub fn clear(mut self) {
        self.end();
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.end();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_without_a_terminal() {
        // Tests run without a terminal: no escape codes.
        assert_eq!(super::pink("x"), "x");
        assert_eq!(super::pill("hi"), "[hi]");
        assert_eq!(super::strip("\x1b[1mab\x1b[0mc"), "abc");
    }
}
