import Foundation

/// Readable text out of an HTML mail body.
///
/// Not a renderer, and deliberately not a web view. A mail body's
/// markup arrives with remote images in it, and every one of those is a
/// request to somebody else's server the moment a message is opened —
/// it reports that the mail was read, when, and from what address. Mail
/// clients call the setting "load remote content" and ship it off.
/// Extracting the text does not have the setting at all.
enum HTMLText {
    /// Blocks that end a line when they close.
    private static let blocks: Set<String> = [
        "p", "div", "br", "tr", "li", "h1", "h2", "h3", "h4", "h5", "h6",
        "blockquote", "table", "ul", "ol", "section", "article", "pre",
    ]

    /// Elements whose contents are not text at all.
    private static let silent: Set<String> = ["script", "style", "head", "title"]

    /// The earlier mail a reply quotes, as the common clients mark it:
    /// Gmail, Apple Mail and Thunderbird nest it in a blockquote or a
    /// marked block. The web's folded rows leave out the same ones.
    private static let quotedClasses = ["gmail_quote", "gmail_extra", "yahoo_quoted", "moz-cite-prefix"]
    /// Outlook does not nest it: everything after its reply header is the
    /// earlier mail.
    private static let outlookReplyIds = ["appendonsend", "divrplyfwdmsg"]

    /// The text, with `dropQuoted` leaving out the earlier mail a reply
    /// quotes: what a folded row shows, where only the new words matter.
    static func plain(_ html: String, dropQuoted: Bool = false) -> String {
        var out = ""
        var tag = ""
        var inTag = false
        var skipUntil: String?
        // the quoted element being skipped, and how deep inside it we are
        var quoted: (name: String, depth: Int)?
        var i = html.startIndex

        while i < html.endIndex {
            let ch = html[i]
            // `<` opens a tag only when what follows could name one.
            // Mail arrives with `a < b`, and with plain text mislabelled
            // as HTML; treating every `<` as a tag eats the rest of the
            // line in both cases.
            if ch == "<", startsATag(html, after: i) {
                inTag = true
                tag = ""
                i = html.index(after: i)
                continue
            }
            if ch == ">" && inTag {
                inTag = false
                let name = tagName(tag)
                if let waiting = skipUntil {
                    if tag.hasPrefix("/") && name == waiting { skipUntil = nil }
                } else if let q = quoted {
                    quoted = deeper(q, tag, name)
                } else if dropQuoted && startsOutlookReply(tag) {
                    break
                } else if dropQuoted && startsQuote(tag, name) {
                    quoted = (name, 1)
                } else if silent.contains(name) && !tag.hasPrefix("/") {
                    // A self-closed `<style/>` never gets its closing
                    // tag, and waiting for one swallows the message.
                    if !tag.hasSuffix("/") { skipUntil = name }
                } else if blocks.contains(name) {
                    if !out.hasSuffix("\n") { out.append("\n") }
                }
                i = html.index(after: i)
                continue
            }
            if inTag {
                tag.append(ch)
                i = html.index(after: i)
                continue
            }
            if skipUntil != nil || quoted != nil {
                i = html.index(after: i)
                continue
            }
            out.append(ch)
            i = html.index(after: i)
        }
        return tidy(entities(out))
    }

    private static func startsQuote(_ tag: String, _ name: String) -> Bool {
        guard !tag.hasPrefix("/"), !tag.hasSuffix("/") else { return false }
        if name == "blockquote" { return true }
        let lower = tag.lowercased()
        return quotedClasses.contains { lower.contains($0) }
    }

    private static func startsOutlookReply(_ tag: String) -> Bool {
        guard !tag.hasPrefix("/") else { return false }
        let lower = tag.lowercased()
        return outlookReplyIds.contains { lower.contains($0) }
    }

    /// The quote after one more tag: deeper for another element of its
    /// name, shallower for a closing one, over when that closes it.
    private static func deeper(
        _ q: (name: String, depth: Int), _ tag: String, _ name: String
    ) -> (name: String, depth: Int)? {
        guard name == q.name, !tag.hasSuffix("/") else { return q }
        if !tag.hasPrefix("/") { return (q.name, q.depth + 1) }
        if q.depth == 1 { return nil }
        return (q.name, q.depth - 1)
    }

    private static func startsATag(_ html: String, after i: String.Index) -> Bool {
        let next = html.index(after: i)
        guard next < html.endIndex else { return false }
        let ch = html[next]
        // `!` for comments and the doctype, which are dropped whole.
        return ch.isLetter || ch == "/" || ch == "!"
    }

    private static func tagName(_ tag: String) -> String {
        var name = tag
        if name.hasPrefix("/") { name.removeFirst() }
        name = String(name.prefix(while: { !$0.isWhitespace && $0 != "/" }))
        return name.lowercased()
    }

    /// The handful that actually appear in mail, plus numeric ones.
    ///
    /// `&nbsp;` becomes an ordinary space rather than U+00A0: a
    /// non-breaking space is invisible and unbreakable, and a paragraph
    /// full of them will not wrap on a phone.
    private static func entities(_ s: String) -> String {
        var out = ""
        var i = s.startIndex
        while i < s.endIndex {
            guard s[i] == "&", let semi = s[i...].firstIndex(of: ";"),
                s.distance(from: i, to: semi) <= 10
            else {
                out.append(s[i])
                i = s.index(after: i)
                continue
            }
            let name = String(s[s.index(after: i)..<semi])
            out.append(decode(name) ?? String(s[i...semi]))
            i = s.index(after: semi)
        }
        return out
    }

    private static func decode(_ name: String) -> String? {
        switch name.lowercased() {
        case "amp": return "&"
        case "lt": return "<"
        case "gt": return ">"
        case "quot": return "\""
        case "apos", "#39": return "'"
        case "nbsp": return " "
        case "mdash": return "—"
        case "ndash": return "–"
        case "hellip": return "…"
        case "rsquo": return "’"
        case "lsquo": return "‘"
        case "ldquo": return "“"
        case "rdquo": return "”"
        default: break
        }
        guard name.hasPrefix("#") else { return nil }
        let digits = name.dropFirst()
        let value: UInt32?
        if digits.hasPrefix("x") || digits.hasPrefix("X") {
            value = UInt32(digits.dropFirst(), radix: 16)
        } else {
            value = UInt32(digits)
        }
        guard let value, let scalar = Unicode.Scalar(value) else { return nil }
        return String(Character(scalar))
    }

    /// Collapse the whitespace markup leaves behind.
    ///
    /// Mail HTML is generated, and generated markup is indented: every
    /// newline and run of spaces between tags is layout, not text.
    ///
    /// **Blank lines go entirely.** Every block already ends its own
    /// line, so paragraphs stay apart without them, and what is left
    /// when they are kept is one blank line per level of indentation in
    /// somebody's template — which is most of the screen on marketing
    /// mail.
    private static func tidy(_ s: String) -> String {
        // A lone CR is a line ending too — old Mac files, and the tail
        // of a message whose last line was never terminated. Swift's
        // `.whitespaces` is space and tab **only**; CR lives in
        // `.newlines`, so trimming with the first leaves it in and the
        // text ends with an invisible character that nothing else
        // accounts for.
        s.replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map {
                $0.split(separator: " ", omittingEmptySubsequences: true)
                    .joined(separator: " ")
                    .trimmingCharacters(in: .whitespacesAndNewlines)
            }
            .filter { !$0.isEmpty }
            .joined(separator: "\n")
    }
}
