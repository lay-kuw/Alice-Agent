//! Streaming Markdown → ANSI renderer for the terminal.
//! Line style is decided from the first few characters of each line, so
//! text keeps streaming instead of waiting for whole lines.

use std::io::Write;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALIC: &str = "\x1b[3m";
const CYAN: &str = "\x1b[36m";
const HEAD: &str = "\x1b[1;35m";

#[derive(PartialEq, Clone, Copy)]
enum Line {
    Normal,
    Header,
    Quote,
    Fence,
    Code,
}

pub struct Md {
    color: bool,
    line_start: bool,
    head: String,
    in_code_block: bool,
    line: Line,
    bold: bool,
    code: bool,
    star: bool,
    pub out: Vec<u8>,
    pub at_line_start: bool,
}

impl Md {
    pub fn new(color: bool) -> Md {
        Md {
            color,
            line_start: true,
            head: String::new(),
            in_code_block: false,
            line: Line::Normal,
            bold: false,
            code: false,
            star: false,
            out: Vec::new(),
            at_line_start: true,
        }
    }

    pub fn push(&mut self, s: &str) {
        if !self.color {
            self.out.extend_from_slice(s.as_bytes());
            if let Some(c) = s.chars().last() {
                self.at_line_start = c == '\n';
            }
            return;
        }
        for c in s.chars() {
            self.ch(c);
        }
    }

    /// Write buffered output to stdout.
    pub fn flush(&mut self) {
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(&self.out);
        let _ = so.flush();
        self.out.clear();
    }

    /// Finish the current line and reset styles.
    pub fn finish(&mut self) {
        if self.line_start && !self.head.is_empty() {
            self.start_line();
        }
        if self.star {
            self.star = false;
            self.raw("*");
        }
        if self.color {
            self.raw(RESET);
        }
        if !self.at_line_start {
            self.raw("\n");
            self.at_line_start = true;
        }
        self.line_start = true;
        self.bold = false;
        self.code = false;
        self.flush();
    }

    fn raw(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    fn ch(&mut self, c: char) {
        if self.line_start {
            self.head.push(c);
            let n = self.head.chars().count();
            let decided = c == '\n' || n >= 4 || (n >= 1 && !matches!(self.head.trim_start().chars().next(), Some('#' | '`' | '>' | '*' | '-' | ' ') | None));
            if decided {
                self.start_line();
            }
            return;
        }
        self.emit(c);
    }

    fn start_line(&mut self) {
        let head = std::mem::take(&mut self.head);
        self.line_start = false;
        let t = head.trim_start();
        if t.starts_with("```") {
            self.in_code_block = !self.in_code_block;
            self.line = Line::Fence;
            self.raw(DIM);
            for c in head.chars() {
                self.emit(c);
            }
            return;
        }
        if self.in_code_block {
            self.line = Line::Code;
            self.raw(CYAN);
            for c in head.chars() {
                self.emit(c);
            }
            return;
        }
        if t.starts_with('#') {
            self.line = Line::Header;
            self.raw(HEAD);
        } else if t.starts_with('>') {
            self.line = Line::Quote;
            self.raw(DIM);
            self.raw(ITALIC);
        } else {
            self.line = Line::Normal;
        }
        for c in head.chars() {
            self.emit(c);
        }
    }

    fn style(&self) -> String {
        let mut s = String::from(RESET);
        match self.line {
            Line::Header => s.push_str(HEAD),
            Line::Quote => {
                s.push_str(DIM);
                s.push_str(ITALIC)
            }
            Line::Fence => s.push_str(DIM),
            Line::Code => s.push_str(CYAN),
            Line::Normal => {}
        }
        if self.bold {
            s.push_str(BOLD);
        }
        if self.code {
            s.push_str(CYAN);
        }
        s
    }

    fn emit(&mut self, c: char) {
        if c == '\n' {
            if self.star {
                self.star = false;
                self.raw("*");
            }
            self.raw(RESET);
            self.raw("\n");
            self.line_start = true;
            self.at_line_start = true;
            self.bold = false;
            self.code = false;
            self.line = Line::Normal;
            return;
        }
        self.at_line_start = false;
        if matches!(self.line, Line::Code | Line::Fence) {
            let mut b = [0u8; 4];
            self.raw(c.encode_utf8(&mut b));
            return;
        }
        if c == '`' {
            self.code = !self.code;
            let st = self.style();
            self.raw(&st);
            return;
        }
        if !self.code && c == '*' {
            if self.star {
                self.star = false;
                self.bold = !self.bold;
                let st = self.style();
                self.raw(&st);
            } else {
                self.star = true;
            }
            return;
        }
        if self.star {
            self.star = false;
            self.raw("*");
        }
        let mut b = [0u8; 4];
        self.raw(c.encode_utf8(&mut b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(chunks: &[&str]) -> String {
        let mut m = Md::new(true);
        for c in chunks {
            m.push(c);
        }
        m.finish_no_flush();
        String::from_utf8(m.out).unwrap()
    }

    impl Md {
        fn finish_no_flush(&mut self) {
            if self.line_start && !self.head.is_empty() {
                self.start_line();
            }
        }
    }

    #[test]
    fn styles_are_applied_across_chunks() {
        let s = render(&["# Ti", "tle\nsome **bo", "ld** and `co", "de`\n```rust\nlet x;\n```\n"]);
        assert!(s.contains(HEAD));
        assert!(s.contains(BOLD));
        assert!(s.contains("let x;"));
        assert!(!s.contains("**"));
        let plain = render(&["a * b\n"]);
        assert!(plain.contains("a * b"));
    }
}
