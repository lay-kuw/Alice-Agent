import Foundation

/// Block-level Markdown split. Inline formatting is rendered by
/// `AttributedString(markdown:)`; code blocks and headings need their own views.
public enum MarkdownBlock: Hashable, Sendable {
    case paragraph(String)
    case heading(level: Int, text: String)
    case code(language: String, code: String)
    case listItem(marker: String, text: String)
    case quote(String)

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
        for raw in src.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n") {
            let line = raw
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
