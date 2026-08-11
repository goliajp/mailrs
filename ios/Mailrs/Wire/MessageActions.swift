import Foundation

/// What "copy" and "share" mean for one message.
///
/// Its own file because both answers are rules, not rendering: what
/// gets copied out of an HTML newsletter is a decision, and a view is
/// the wrong place to make one that cannot be tested.
enum MessageActions {
    /// The body as text a person can paste.
    ///
    /// The plain part when the sender wrote one, and the HTML stripped
    /// when they did not — pasting markup into a chat window is not
    /// what anybody meant by "copy". An empty answer is possible and
    /// honest: nine bodies in a 900-message sample carry nothing at
    /// all.
    static func plainText(_ message: Wire.Message) -> String {
        if let text = message.textBody, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return text
        }
        guard let html = message.htmlBody else { return "" }
        return stripped(html)
    }

    /// Subject and body together, which is what a share sheet should
    /// hand on — a body alone arrives at the other end with no idea
    /// what it is.
    static func shareable(_ message: Wire.Message) -> String {
        let body = plainText(message)
        let subject = message.subject.trimmingCharacters(in: .whitespacesAndNewlines)
        if subject.isEmpty { return body }
        if body.isEmpty { return subject }
        return "\(subject)\n\n\(body)"
    }

    /// Tags out, entities back, runs of blank collapsed.
    ///
    /// Not a parser and not trying to be: `<script>` and `<style>` go
    /// whole, because their contents are code and would otherwise
    /// arrive in the clipboard as text.
    static func stripped(_ html: String) -> String {
        var text = html
        for tag in ["script", "style"] {
            text = text.replacingOccurrences(
                of: "<\(tag)[^>]*>.*?</\(tag)>", with: " ",
                options: [.regularExpression, .caseInsensitive])
        }
        // A block boundary is a line break to a reader, and without
        // this a newsletter arrives as one unbroken paragraph.
        text = text.replacingOccurrences(
            of: "</(p|div|tr|h[1-6])>|<br[^>]*>", with: "\n",
            options: [.regularExpression, .caseInsensitive])
        text = text.replacingOccurrences(
            of: "<[^>]+>", with: "", options: .regularExpression)
        for (entity, character) in [
            ("&nbsp;", " "), ("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"),
            ("&quot;", "\""), ("&#39;", "'"), ("&apos;", "'"),
        ] {
            text = text.replacingOccurrences(of: entity, with: character, options: .caseInsensitive)
        }
        // A table-based layout leaves runs of breaks behind — empty
        // `<p></p>` pairs become bare newlines with nothing between
        // them, which the first version of this rule missed because it
        // asked for blank *lines* and there was no line at all.
        text = text.replacingOccurrences(
            of: "[ \t]*\n[ \t\n]*\n[ \t\n]*", with: "\n\n", options: .regularExpression)
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
