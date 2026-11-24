import Foundation

/// Block-level Markdown split. Inline formatting is rendered by
/// `AttributedString(markdown:)`; code blocks and headings need their own views.
public enum MarkdownBlock: Hashable, Sendable {
    case paragraph(String)
    case heading(level: Int, text: String)
    case code(language: String, code: String)
    case listItem(marker: String, text: String)
    case quote(String)
    /// GitHub-style table. `align` has one entry per column.
    case table(header: [String], align: [TableAlign], rows: [[String]])

    public static func parse(_ src: String) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        var para: [String] = []
        var code: [String]? = nil
        var lang = ""
        func flush() {
            if !para.isEmpty {
                blocks.append(.paragraph(para.joined(separator: "\n")))
                para.removeAll()
            }
        }
        let lines = src.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
        var i = 0
        while i < lines.count {
            let line = lines[i]
            i += 1
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("```") {
                if let c = code {
                    blocks.append(.code(language: lang, code: c.joined(separator: "\n")))
                    code = nil
                } else {
                    flush()
                    lang = String(trimmed.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                    code = []
                }
                continue
            }
            if code != nil {
                code!.append(line)
                continue
            }
            if trimmed.isEmpty {
                flush()
                continue
            }
            // A table: a row of cells followed by a |---|:--:| separator.
            if trimmed.contains("|"), i < lines.count, let align = separator(lines[i]) {
                flush()
                let header = cells(trimmed)
                var rows: [[String]] = []
                i += 1
                while i < lines.count {
                    let t = lines[i].trimmingCharacters(in: .whitespaces)
                    guard !t.isEmpty, t.contains("|") else { break }
                    var r = cells(t)
                    // Pad / cut to the header width.
                    if r.count < header.count { r += Array(repeating: "", count: header.count - r.count) }
                    rows.append(Array(r.prefix(header.count)))
                    i += 1
                }
                let a = align.count >= header.count ? Array(align.prefix(header.count)) : align + Array(repeating: .left, count: header.count - align.count)
                blocks.append(.table(header: header, align: a, rows: rows))
                continue
            }
            if let h = heading(trimmed) {
                flush()
                blocks.append(h)
                continue
            }
            if let li = listItem(trimmed) {
                flush()
                blocks.append(li)
                continue
            }
            if trimmed.hasPrefix(">") {
                flush()
                blocks.append(.quote(String(trimmed.dropFirst()).trimmingCharacters(in: .whitespaces)))
                continue
            }
            para.append(line)
        }
        if let c = code { blocks.append(.code(language: lang, code: c.joined(separator: "\n"))) }
        flush()
        return blocks
    }

    /// Cells of a table row: `| a | b |` or `a | b`; `\|` stays a literal bar.
    static func cells(_ s: String) -> [String] {
        var t = s.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("|") { t.removeFirst() }
        if t.hasSuffix("|") && !t.hasSuffix("\\|") { t.removeLast() }
        var out: [String] = []
        var cur = ""
        var escaped = false
        for c in t {
            if escaped { cur.append(c); escaped = false; continue }
            if c == "\\" { escaped = true; continue }
            if c == "|" { out.append(cur.trimmingCharacters(in: .whitespaces)); cur = ""; continue }
            cur.append(c)
        }
        out.append(cur.trimmingCharacters(in: .whitespaces))
        return out
    }

    /// Column alignments if `s` is a separator row like `|:---|:-:|--:|`.
    static func separator(_ s: String) -> [TableAlign]? {
        let parts = cells(s)
        guard !parts.isEmpty else { return nil }
        var out: [TableAlign] = []
        for p in parts {
            guard p.count >= 1, p.allSatisfy({ $0 == "-" || $0 == ":" }), p.contains("-") else { return nil }
            switch (p.hasPrefix(":"), p.hasSuffix(":")) {
            case (true, true): out.append(.center)
            case (false, true): out.append(.right)
            default: out.append(.left)
            }
        }
        return out
    }

    static func heading(_ s: String) -> MarkdownBlock? {
        let hashes = s.prefix(while: { $0 == "#" }).count
        guard (1...6).contains(hashes), s.dropFirst(hashes).first == " " else { return nil }
        return .heading(level: hashes, text: String(s.dropFirst(hashes + 1)))
    }

    static func listItem(_ s: String) -> MarkdownBlock? {
        for m in ["- ", "* ", "+ "] where s.hasPrefix(m) {
            return .listItem(marker: "•", text: String(s.dropFirst(2)))
        }
        let digits = s.prefix(while: { $0.isNumber })
        if !digits.isEmpty, s.dropFirst(digits.count).hasPrefix(". ") {
            return .listItem(marker: "\(digits).", text: String(s.dropFirst(digits.count + 2)))
        }
        return nil
    }

    /// Inline Markdown as an attributed string (links, bold, code). Raw HTML is not interpreted.
    public static func inline(_ s: String) -> AttributedString {
        let opts = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace, failurePolicy: .returnPartiallyParsedIfPossible)
        var a = (try? AttributedString(markdown: s, options: opts)) ?? AttributedString(s)
        // Only allow web links.
        for run in a.runs {
            if let url = run.link, !["http", "https"].contains(url.scheme?.lowercased() ?? "") {
                a[run.range].link = nil
            }
        }
        return a
    }
}

public enum TableAlign: String, Hashable, Sendable {
    case left, center, right
}
