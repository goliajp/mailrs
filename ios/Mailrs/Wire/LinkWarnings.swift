import Foundation

/// Marks beside the links the server flagged, and the question asked
/// before one opens.
///
/// The server reads the message as it was stored and names the hosts
/// (`link_warnings` on each message); this finds the matching anchors.
/// The rule is the web client's: an anchor falls under a warning when it
/// goes to the warning's host and, for a warning about the text showing
/// another host, its text shows that host.
///
/// The body is rendered with scripting off, so the mark and the question
/// cannot live in the page. Instead a flagged anchor's target is swapped
/// for `mailrs-link-warning:<n>` and the real one kept here; the body
/// view hands that address back, and the reading screen asks first.
enum LinkWarnings {
    static let scheme = "mailrs-link-warning"

    /// The body with its flagged anchors marked, and what each mark
    /// stands for.
    struct Marked: Equatable {
        let html: String
        /// By the `<n>` in `mailrs-link-warning:<n>`.
        let targets: [Target]
    }

    struct Target: Equatable {
        let url: URL
        let warning: Wire.LinkWarning
    }

    /// The warning an anchor going to `href` with the text `text` falls under.
    static func warning(
        href: String, text: String, in warnings: [Wire.LinkWarning]
    ) -> Wire.LinkWarning? {
        guard let host = URL(string: href)?.host()?.lowercased() else { return nil }
        let shownText = text.filter { !$0.isWhitespace }.lowercased()
        return warnings.first { w in
            guard w.host == host else { return false }
            guard let shown = w.shown else { return true }
            return shownText.contains(shown)
        }
    }

    /// The index a body link stands for, when it is one of the marks.
    static func index(of url: URL) -> Int? {
        guard url.scheme == scheme else { return nil }
        return Int(url.absoluteString.dropFirst(scheme.count + 1))
    }

    private static let anchor = try? NSRegularExpression(
        pattern: #"<a\b([^>]*)>(.*?)</a\s*>"#,
        options: [.caseInsensitive, .dotMatchesLineSeparators])
    private static let hrefAttr = try? NSRegularExpression(
        pattern: #"\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#,
        options: [.caseInsensitive])
    private static let tag = try? NSRegularExpression(pattern: "<[^>]*>")

    /// Mark every flagged anchor in `html`. `describe` words a warning's
    /// reasons; the words become the mark's accessible label.
    static func mark(
        html: String, warnings: [Wire.LinkWarning], describe: (Wire.LinkWarning) -> String
    ) -> Marked {
        guard !warnings.isEmpty, let anchor, let hrefAttr else {
            return Marked(html: html, targets: [])
        }
        let source = html as NSString
        var out = ""
        var targets: [Target] = []
        var cursor = 0
        for match in anchor.matches(in: html, range: NSRange(location: 0, length: source.length)) {
            let attrs = source.substring(with: match.range(at: 1))
            let inner = source.substring(with: match.range(at: 2))
            let attrsNS = attrs as NSString
            guard let h = hrefAttr.firstMatch(
                in: attrs, range: NSRange(location: 0, length: attrsNS.length))
            else { continue }
            let valueRange = [1, 2, 3].map { h.range(at: $0) }.first { $0.location != NSNotFound }
            guard let valueRange else { continue }
            let href = entities(attrsNS.substring(with: valueRange))
            guard let w = warning(href: href, text: plain(inner), in: warnings),
                  let url = URL(string: href)
            else { continue }
            let n = targets.count
            targets.append(Target(url: url, warning: w))
            let rewritten = attrsNS.replacingCharacters(in: h.range, with: "href=\"\(scheme):\(n)\"")
            let label = escape(describe(w))
            out += source.substring(with: NSRange(location: cursor, length: match.range.location - cursor))
            out += "<a\(rewritten)>\(inner)</a>"
            out += "<a href=\"\(scheme):\(n)\" class=\"mailrs-link-warning\" role=\"img\""
                + " aria-label=\"\(label)\" title=\"\(label)\" style=\"\(markStyle)\">!</a>"
            cursor = match.range.location + match.range.length
        }
        guard !targets.isEmpty else { return Marked(html: html, targets: []) }
        out += source.substring(from: cursor)
        return Marked(html: out, targets: targets)
    }

    // the body's own stylesheet does not know the mark, so it carries its look
    private static let markStyle =
        "display:inline-block;margin-left:2px;padding:0 4px;border-radius:3px;"
        + "background:#dc2626;color:#fff;font:600 10px/14px sans-serif;"
        + "text-decoration:none;vertical-align:1px"

    /// An anchor's text as a reader sees it.
    private static func plain(_ inner: String) -> String {
        guard let tag else { return inner }
        let ns = inner as NSString
        return entities(tag.stringByReplacingMatches(
            in: inner, range: NSRange(location: 0, length: ns.length), withTemplate: ""))
    }

    private static func entities(_ s: String) -> String {
        s.replacingOccurrences(of: "&lt;", with: "<")
            .replacingOccurrences(of: "&gt;", with: ">")
            .replacingOccurrences(of: "&quot;", with: "\"")
            .replacingOccurrences(of: "&#39;", with: "'")
            .replacingOccurrences(of: "&nbsp;", with: " ")
            .replacingOccurrences(of: "&amp;", with: "&")
    }

    private static func escape(_ s: String) -> String {
        s.replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\"", with: "&quot;")
    }
}
