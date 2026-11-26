//! Look of the interactive CLI (setup, models, logins): soft pastel colors,
//! a rounded title card, numbered steps and choices with short notes.
//! Without a color terminal (NO_COLOR, pipes) the same layout prints plain.

use super::chat::color;
use unicode_width::UnicodeWidthStr;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const LAVENDER: &str = "\x1b[38;2;184;166;245m";
const PINK: &str = "\x1b[38;2;247;158;199m";
const MINT: &str = "\x1b[38;2;140;219;191m";
const PEACH: &str = "\x1b[38;2;255;178;138m";
const ROSE: &str = "\x1b[38;2;240;120;133m";

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
pub fn bold(s: &str) -> String {
    paint(&[BOLD], s)
}
pub fn dim(s: &str) -> String {
    paint(&[DIM], s)
}

/// Longest text a card holds on one line.
const CARD_MAX: usize = 64;

/// A path with the home folder as `~`.
pub fn path(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

/// Shortened in the middle to fit `max` columns.
fn fit(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let keep = max.saturating_sub(1) / 2;
    let head: String = chars[..keep].iter().collect();
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("{head}…{tail}")
}

/// A rounded card with a title and a few dim lines under it.
pub fn card(title: &str, lines: &[String]) {
    let lines: Vec<String> = lines.iter().map(|l| fit(l, CARD_MAX)).collect();
    let inner = std::iter::once(title.width())
        .chain(lines.iter().map(|l| l.width()))
        .max()
        .unwrap_or(0)
        .clamp(32, CARD_MAX)
        + 2;
    let edge = |s: &str| lavender(s);
    println!();
    println!("  {}", edge(&format!("╭{}╮", "─".repeat(inner + 2))));
    let row = |text: String, w: usize| println!("  {} {}{} {}", edge("│"), text, " ".repeat(inner.saturating_sub(w)), edge("│"));
    row(paint(&[BOLD, PINK], title), title.width());
    for l in &lines {
        row(dim(l), l.width());
    }
    println!("  {}", edge(&format!("╰{}╯", "─".repeat(inner + 2))));
}

/// "◆ 2/5  Server  ●●○○○"
pub fn step(n: usize, total: usize, title: &str) {
    let dots: String = (1..=total).map(|i| if i <= n { "●" } else { "○" }).collect();
    println!();
    println!("  {} {}  {}  {}", pink("◆"), dim(&format!("{n}/{total}")), bold(title), lavender(&dots));
}

/// A section heading without a number.
pub fn heading(title: &str) {
    println!();
    println!("  {} {}", pink("◆"), bold(title));
}

/// Dim explanation lines under a step, behind a soft gutter.
pub fn note(text: &str) {
    for l in text.lines() {
        println!("  {} {}", lavender("│"), dim(l));
    }
}

pub fn ok(text: &str) {
    println!("  {} {text}", mint("✓"));
}
pub fn warn(text: &str) {
    println!("  {} {text}", peach("!"));
}
pub fn fail(text: &str) {
    println!("  {} {text}", rose("✗"));
}

/// The question line: "  ? Port [7878] › "
pub fn question(prompt: &str, default: &str) -> String {
    let d = if default.is_empty() { String::new() } else { format!(" {}", dim(&format!("[{default}]"))) };
    format!("  {} {}{d} {} ", lavender("?"), prompt.trim(), pink("›"))
}

/// A numbered option: "   1  openai   API key · GPT"
pub fn option(n: usize, label: &str, note: &str, width: usize, selected: bool) {
    let num = if selected { pink(&format!("{n:>2}")) } else { lavender(&format!("{n:>2}")) };
    let pad = " ".repeat(width.saturating_sub(label.width()));
    let label = if selected { bold(label) } else { label.to_string() };
    if note.is_empty() {
        println!("   {num}  {label}");
    } else {
        println!("   {num}  {label}{pad}  {}", dim(note));
    }
}

/// The closing card with what to do next.
pub fn done(title: &str, next: &[(&str, &str)]) {
    let w = next.iter().map(|(c, _)| c.width()).max().unwrap_or(0);
    let lines: Vec<String> = next.iter().map(|(c, d)| format!("{c}{}  {d}", " ".repeat(w - c.width()))).collect();
    card(title, &lines);
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_without_a_terminal() {
        // Tests run without a terminal: no escape codes.
        assert_eq!(super::pink("x"), "x");
        assert_eq!(super::question("  Port", "7878"), "  ? Port [7878] › ");
        let long = "x".repeat(100);
        assert_eq!(super::fit(&long, 21).chars().count(), 21);
    }
}
