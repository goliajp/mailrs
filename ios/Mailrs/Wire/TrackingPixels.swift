import Foundation

/// The images in a received message that only tell the sender it was read.
///
/// The web's rule, from the same `trackers.json` the server and the web
/// read (a copy ships in the app; `check-tracker-parity.sh` keeps the bytes
/// identical): an `http(s)` image is a tracker when its host or path is a
/// known tracking one, or when it is invisible — hidden by style, or every
/// side it states is a pixel or less.
///
/// Removed from the document, not merely blocked, so "load images" is a
/// decision about pictures and not about being counted.
enum TrackingPixels {
    struct Rules: Decodable {
        let hosts: [String]
        let paths: [String]
    }

    static let rules: Rules = {
        guard let url = Bundle.main.url(forResource: "trackers", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let rules = try? JSONDecoder().decode(Rules.self, from: data)
        else { fatalError("the app does not ship a readable trackers.json") }
        return rules
    }()

    /// The document with its trackers taken out.
    static func strip(html: String) -> String {
        guard html.range(of: "<img", options: .caseInsensitive) != nil else { return html }
        var out = ""
        out.reserveCapacity(html.utf8.count)
        var rest = Substring(html)
        while let open = rest.range(of: "<img", options: .caseInsensitive) {
            out += rest[..<open.lowerBound]
            let after = rest[open.lowerBound...]
            guard let close = tagEnd(after) else {
                out += after
                return out
            }
            let tag = after[...close]
            if !isTracker(String(tag)) { out += tag }
            rest = after[after.index(after: close)...]
        }
        out += rest
        return out
    }

    /// How many trackers `strip` takes out, for the note above the message.
    static func count(html: String) -> Int {
        var n = 0
        var rest = Substring(html)
        while let open = rest.range(of: "<img", options: .caseInsensitive) {
            let after = rest[open.lowerBound...]
            guard let close = tagEnd(after) else { return n }
            if isTracker(String(after[...close])) { n += 1 }
            rest = after[after.index(after: close)...]
        }
        return n
    }

    /// Whether one `<img …>` tag is a tracker.
    static func isTracker(_ tag: String) -> Bool {
        let attrs = attributes(tag)
        let src = (attrs["src"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard src.range(of: #"^https?://"#, options: [.regularExpression, .caseInsensitive]) != nil
        else { return false }
        return trackingURL(src) || invisible(attrs)
    }

    /// The `>` that closes the tag, skipping any inside a quoted value.
    private static func tagEnd(_ s: Substring) -> Substring.Index? {
        var quote: Character?
        for i in s.indices {
            let c = s[i]
            if let q = quote {
                if c == q { quote = nil }
            } else if c == "\"" || c == "'" {
                quote = c
            } else if c == ">" {
                return i
            }
        }
        return nil
    }

    /// Attribute names lowercased; the first of a repeated name wins, as in a browser.
    private static func attributes(_ tag: String) -> [String: String] {
        let inner = tag.dropFirst(4).dropLast()
        let pattern = #"([^\s"'>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return [:] }
        let text = String(inner)
        var out: [String: String] = [:]
        for m in regex.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
            guard let nameRange = Range(m.range(at: 1), in: text) else { continue }
            let name = text[nameRange].lowercased()
            if out[name] != nil { continue }
            var value = ""
            for group in 2...4 {
                if let r = Range(m.range(at: group), in: text) { value = String(text[r]) }
            }
            out[name] = value
        }
        return out
    }

    private static func trackingURL(_ src: String) -> Bool {
        guard let parts = URLComponents(string: src), let rawHost = parts.host else { return false }
        let host = rawHost.lowercased()
        let path = parts.percentEncodedPath.lowercased()
        if rules.hosts.contains(where: { host == $0 || host.hasSuffix("." + $0) }) { return true }
        return rules.paths.contains { path.contains($0) }
    }

    /// Drawn at a pixel or less on each side it states, or hidden outright.
    private static func invisible(_ attrs: [String: String]) -> Bool {
        let style = (attrs["style"] ?? "").lowercased().filter { !$0.isWhitespace }
        if style.contains("display:none") || style.contains("visibility:hidden") { return true }
        let sides = ["width", "height"].compactMap { side in
            px(attrs[side]) ?? fromStyle(style, side)
        }
        return !sides.isEmpty && sides.allSatisfy { $0 <= 1 }
    }

    private static func fromStyle(_ style: String, _ side: String) -> Int? {
        firstNumber(style, #"(?:^|;)\#(side):(\d+)(px)?(?:;|$)"#, options: [])
    }

    /// `1`, `1px`, `0`; nil for anything relative or absent.
    private static func px(_ value: String?) -> Int? {
        guard let value else { return nil }
        return firstNumber(value, #"^\s*(\d+)\s*(px)?\s*$"#, options: [.caseInsensitive])
    }

    private static func firstNumber(
        _ text: String, _ pattern: String, options: NSRegularExpression.Options
    ) -> Int? {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: options),
              let m = regex.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)),
              let r = Range(m.range(at: 1), in: text)
        else { return nil }
        return Int(text[r])
    }
}
