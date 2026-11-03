import Foundation

/// The composer's markdown as the HTML a send puts in `html_body`, the way
/// the web's send path renders it (`renderMarkdownForSend` in
/// `web/src/lib/render-markdown.ts`), held to the same bytes by the shared
/// `markdown-samples.json`.
///
/// The subset a phone composer writes or a reply carries: paragraphs and line
/// breaks, headings, quotes, simple lists, bold, italic, code, links and bare
/// URLs and addresses. Other markdown (tables, fenced code, nested lists)
/// stays text. What a person types is never markup: `<` and `&` arrive
/// escaped, and a link is a link only for http, https and mailto.
enum MarkdownHtml {
    /// `html_body` for a body, or nil when it has no bold, italic or link of
    /// its own — then the message goes as text alone, as it always has.
    static func forSend(_ body: String, wrapped: Bool = true) -> String? {
        guard hasMarks(body) else { return nil }
        let html = render(body)
        if wrapped { return wrap(html) }
        return html
    }

    static func hasMarks(_ body: String) -> Bool {
        let probe = inline(body, autolinks: false)
        return probe.contains("<strong>") || probe.contains("<em>") || probe.contains("<a ")
    }

    /// The whole body, blank lines kept as empty paragraphs.
    static func render(_ markdown: String) -> String {
        blocks(preserveBlankLines(markdown.replacingOccurrences(of: "\r\n", with: "\n")))
    }

    static func wrap(_ body: String) -> String {
        templateHead + body + templateTail
    }

    // MARK: blocks

    private enum Open {
        case none
        case paragraph([String])
        case quote([String])
        case list(ordered: Bool, start: Int, marker: String, items: [String])
    }

    private static func blocks(_ text: String) -> String {
        var out = ""
        var open = Open.none
        func close() {
            out += finish(open)
            open = .none
        }
        for line in text.components(separatedBy: "\n") {
            if line.trimmingCharacters(in: .whitespaces).isEmpty {
                close()
                continue
            }
            if let heading = heading(line) {
                close()
                out += heading
                continue
            }
            if let quoted = quoteLine(line) {
                if case .quote(let lines) = open {
                    open = .quote(lines + [quoted])
                } else {
                    close()
                    open = .quote([quoted])
                }
                continue
            }
            if let item = listItem(line) {
                if case .list(let ordered, let start, let marker, let items) = open, marker == item.marker {
                    open = .list(ordered: ordered, start: start, marker: marker, items: items + [item.text])
                    continue
                }
                if case .paragraph = open, item.ordered, item.start != 1 {
                    open = appendLine(open, line)
                    continue
                }
                close()
                open = .list(ordered: item.ordered, start: item.start, marker: item.marker, items: [item.text])
                continue
            }
            open = appendLine(open, line)
        }
        close()
        return out
    }

    // a line that starts nothing: the paragraph, quote or last item it continues
    private static func appendLine(_ open: Open, _ line: String) -> Open {
        switch open {
        case .none:
            return .paragraph([line])
        case .paragraph(let lines):
            return .paragraph(lines + [line])
        case .quote(let lines):
            return .quote(lines + [line])
        case .list(let ordered, let start, let marker, var items):
            items[items.count - 1] += "\n" + line
            return .list(ordered: ordered, start: start, marker: marker, items: items)
        }
    }

    private static func finish(_ open: Open) -> String {
        switch open {
        case .none:
            return ""
        case .paragraph(let lines):
            return "<p>" + breaks(inline(lines.joined(separator: "\n"))) + "</p>\n"
        case .quote(let lines):
            return "<blockquote>\n" + blocks(lines.joined(separator: "\n")) + "</blockquote>\n"
        case .list(let ordered, let start, _, let items):
            var tag = "ul"
            var openTag = "<ul>"
            if ordered {
                tag = "ol"
                openTag = "<ol>"
                if start != 1 { openTag = "<ol start=\"\(start)\">" }
            }
            let body = items.map { "<li>" + breaks(inline($0)) + "</li>\n" }.joined()
            return openTag + "\n" + body + "</\(tag)>\n"
        }
    }

    private static func heading(_ line: String) -> String? {
        guard let m = firstMatch(#"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$"#, line) else { return nil }
        let level = m[1].count
        return "<h\(level)>" + inline(m[2]) + "</h\(level)>\n"
    }

    private static func quoteLine(_ line: String) -> String? {
        guard let m = firstMatch(#"^ {0,3}> ?(.*)$"#, line) else { return nil }
        return m[1]
    }

    private struct Item {
        let ordered: Bool
        let start: Int
        let marker: String
        let text: String
    }

    private static func listItem(_ line: String) -> Item? {
        if let m = firstMatch(#"^ {0,3}([-*+])[ \t]+(.*)$"#, line) {
            return Item(ordered: false, start: 1, marker: m[1], text: m[2])
        }
        if let m = firstMatch(#"^ {0,3}(\d{1,9})([.)])[ \t]+(.*)$"#, line) {
            return Item(ordered: true, start: Int(m[1]) ?? 1, marker: m[2], text: m[3])
        }
        return nil
    }

    private static func breaks(_ html: String) -> String {
        html.replacingOccurrences(of: "\n", with: "<br>")
    }

    private static func preserveBlankLines(_ md: String) -> String {
        replace(#"\n{2,}"#, in: md) { m in
            "\n\n" + String(repeating: "&nbsp;\n\n", count: m[0].count - 1)
        }
    }

    // MARK: inline

    /// One run of text: links, code and escapes first, held aside as
    /// placeholders so the emphasis rules cannot reach into them.
    static func inline(_ text: String, autolinks: Bool = true) -> String {
        var held: [String] = []
        func hold(_ html: String) -> String {
            held.append(html)
            return "\u{E000}\(held.count - 1)\u{E001}"
        }
        var s = replace(#"\\([!-/:-@\[-`{-~])"#, in: text) { hold(escape($0[1])) }
        s = replace(#"`([^`]+)`"#, in: s) { hold("<code>" + escape($0[1]) + "</code>") }
        s = replace(#"\[([^\]]*)\]\(((?:[^()\s]|\([^()\s]*\))*)\)"#, in: s) { m in
            let label = inline(m[1], autolinks: false)
            guard isSafe(m[2]) else { return hold(label) }
            return hold("<a href=\"" + escape(encodeURI(m[2])) + "\">" + label + "</a>")
        }
        if autolinks {
            s = replace(#"(?<![\w/@.])((?:https?://|www\.)[^\s<\x{E000}\x{E001}]+)"#, in: s) { m in
                let (url, rest) = trimURL(m[1])
                var href = url
                if url.hasPrefix("www.") { href = "http://" + url }
                return hold("<a href=\"" + escape(encodeURI(href)) + "\">" + escape(url) + "</a>") + rest
            }
            s = replace(#"(?<![\w.+-])([A-Za-z0-9._+-]+@[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)+)"#, in: s) { m in
                let (address, rest) = trimAddress(m[1])
                return hold("<a href=\"mailto:" + escape(encodeURI(address)) + "\">" + escape(address) + "</a>") + rest
            }
        }
        s = escape(s)
        s = replace(#"\*\*(?=\S)(.+?)(?<=\S)\*\*"#, in: s) { "<strong>" + $0[1] + "</strong>" }
        s = replace(#"(?<![A-Za-z0-9])__(?=\S)(.+?)(?<=\S)__(?![A-Za-z0-9])"#, in: s) { "<strong>" + $0[1] + "</strong>" }
        s = replace(#"\*(?=\S)(.+?)(?<=\S)\*"#, in: s) { "<em>" + $0[1] + "</em>" }
        s = replace(#"(?<![A-Za-z0-9])_(?=\S)(.+?)(?<=\S)_(?![A-Za-z0-9])"#, in: s) { "<em>" + $0[1] + "</em>" }
        // held pieces are finished HTML (a link's label was rendered whole),
        // so one pass puts every one back
        return replace(#"\x{E000}(\d+)\x{E001}"#, in: s) { held[Int($0[1]) ?? 0] }
    }

    private static func isSafe(_ href: String) -> Bool {
        let lower = href.lowercased()
        return lower.hasPrefix("http:") || lower.hasPrefix("https:") || lower.hasPrefix("mailto:")
    }

    // GFM drops trailing punctuation from a bare URL, and a closing paren it did not open
    private static func trimURL(_ raw: String) -> (String, String) {
        var url = raw
        var rest = ""
        while let last = url.last {
            let opens = url.filter { $0 == "(" }.count
            let closes = url.filter { $0 == ")" }.count
            if "?!.,:*_~'\"".contains(last) || (last == ")" && closes > opens) {
                rest = String(last) + rest
                url.removeLast()
                continue
            }
            break
        }
        return (url, rest)
    }

    private static func trimAddress(_ raw: String) -> (String, String) {
        var address = raw
        var rest = ""
        while let last = address.last, ".-_".contains(last) {
            rest = String(last) + rest
            address.removeLast()
        }
        return (address, rest)
    }

    /// `&` (unless it starts an entity), `<`, `>`, `"`, `'` — what marked escapes.
    static func escape(_ text: String) -> String {
        var s = replace(#"&(?!#?\w+;)"#, in: text) { _ in "&amp;" }
        s = s.replacingOccurrences(of: "<", with: "&lt;")
        s = s.replacingOccurrences(of: ">", with: "&gt;")
        s = s.replacingOccurrences(of: "\"", with: "&quot;")
        return s.replacingOccurrences(of: "'", with: "&#39;")
    }

    /// JavaScript's `encodeURI`, keeping `%` as marked does.
    static func encodeURI(_ href: String) -> String {
        let allowed = CharacterSet(charactersIn: uriSafe)
        return href.addingPercentEncoding(withAllowedCharacters: allowed) ?? href
    }

    private static let uriSafe =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789;,/?:@&=+$-_.!~*'()#%"

    // MARK: regex

    private static func firstMatch(_ pattern: String, _ text: String) -> [String]? {
        guard let regex = try? NSRegularExpression(pattern: pattern),
              let m = regex.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) else { return nil }
        return groups(m, in: text)
    }

    private static func replace(_ pattern: String, in text: String, with transform: ([String]) -> String) -> String {
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return text }
        var out = ""
        var cursor = text.startIndex
        for m in regex.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
            guard let range = Range(m.range, in: text) else { continue }
            out += text[cursor..<range.lowerBound]
            out += transform(groups(m, in: text))
            cursor = range.upperBound
        }
        out += text[cursor...]
        return out
    }

    private static func groups(_ m: NSTextCheckingResult, in text: String) -> [String] {
        (0..<m.numberOfRanges).map { i in
            guard let r = Range(m.range(at: i), in: text) else { return "" }
            return String(text[r])
        }
    }

    // the email wrapper, as web/src/components/composer/renderers/email-template.ts writes it
    private static let templateHead = """
    <!DOCTYPE html>
    <html lang="en">
    <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
      body { margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; font-size: 14px; line-height: 1.6; color: #222; background: #fff; }
      a { color: #0066cc; }
      pre { white-space: pre-wrap; word-break: break-word; }
      img { max-width: 100%; height: auto; }
      @media (prefers-color-scheme: dark) {
        body { background: #1a1a1a; color: #e0e0e0; }
        a { color: #5b9cf5; }
        blockquote { border-left-color: #555 !important; }
        hr { border-top-color: #444 !important; }
      }
    </style>
    </head>
    <body>
    <div style="max-width:600px;margin:0 auto;padding:16px">

    """
    private static let templateTail = """

    </div>
    </body>
    </html>
    """
}
