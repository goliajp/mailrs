import Foundation

/// Where a reply stops being new and starts being the letter it
/// answers.
///
/// Measured over 897 real bodies from the production mailbox: 25 of
/// them (2.8%) carry quoted history, and in those the quote is a
/// **median 81%** of the whole body — the worst quartile is 92%. A
/// mailbox this shape is mostly notifications, and the few
/// person-to-person threads in it are exactly the ones worth reading
/// comfortably.
///
/// Low coverage, high payoff, and the trade that keeps it honest: this
/// only ever *splits*. Nothing is rewritten, nothing is dropped, and
/// when no boundary is found the answer is the body unchanged. A
/// reader who taps to expand sees exactly the bytes that arrived.
enum QuotedHistory {
    struct Split: Equatable {
        /// What this sender wrote.
        let body: String
        /// What they were answering, or `nil` when there is none.
        let quoted: String?
    }

    /// `On <date>, <name> wrote:` and the shapes other clients write.
    ///
    /// Anchored to a whole line and bounded at 200 characters: the
    /// attribution is one line, and an unbounded match would let a
    /// paragraph ending in the word "wrote:" swallow the rest of the
    /// message.
    private static let markers: [NSRegularExpression] = {
        let patterns = [
            // Apple Mail, Gmail, and this client's own replies.
            #"^.{0,200}\bwrote:[ \t]*$"#,
            // Japanese clients — the same sentence, and common in this
            // mailbox.
            #"^.{0,200}(さんは、.*書きました|が書きました)[ \t]*[:：]?[ \t]*$"#,
            // Outlook.
            #"^[ \t]*-{4,}[ \t]*Original Message[ \t]*-{4,}[ \t]*$"#,
            // A forward, from almost everything.
            #"^[ \t]*-+[ \t]*Forwarded message[ \t]*-+[ \t]*$"#,
        ]
        return patterns.compactMap {
            try? NSRegularExpression(pattern: $0, options: [.anchorsMatchLines, .caseInsensitive])
        }
    }()

    /// Split `text` at the first thing that says "what follows is
    /// older".
    ///
    /// The attribution line goes with the **quote**, not the body: it
    /// is the quote's title, and a reply that ends on "On Tuesday,
    /// Alice wrote:" with nothing after it reads like a sentence cut
    /// in half.
    static func split(_ text: String) -> Split {
        guard let boundary = boundary(in: text) else {
            return Split(body: text, quoted: nil)
        }
        let body = String(text[text.startIndex..<boundary])
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let quoted = String(text[boundary...])
            .trimmingCharacters(in: .whitespacesAndNewlines)
        // A boundary at the very top means the whole message is quote —
        // a bare forward with no note on it. Folding that leaves an
        // empty card, so leave it alone.
        guard !body.isEmpty, !quoted.isEmpty else {
            return Split(body: text, quoted: nil)
        }
        return Split(body: body, quoted: quoted)
    }

    /// The earliest marker, or the first of a run of `>` lines.
    private static func boundary(in text: String) -> String.Index? {
        var earliest: String.Index?
        let whole = NSRange(text.startIndex..<text.endIndex, in: text)
        for marker in markers {
            guard let match = marker.firstMatch(in: text, range: whole),
                  let found = Range(match.range, in: text)
            else { continue }
            if earliest == nil || found.lowerBound < earliest! {
                earliest = found.lowerBound
            }
        }
        if let run = quotedRun(in: text), earliest == nil || run < earliest! {
            earliest = run
        }
        return earliest
    }

    /// Where a run of at least three `>`-prefixed lines begins.
    ///
    /// Three, not one: a single `>` line is a quotation inside a
    /// sentence as often as it is history, and folding away one line
    /// saves nothing while risking the reader's own words.
    private static func quotedRun(in text: String) -> String.Index? {
        var runStart: String.Index?
        var runLength = 0
        var lineStart = text.startIndex
        for line in text.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline) {
            let isQuote = line.hasPrefix(">")
            if isQuote {
                if runLength == 0 { runStart = lineStart }
                runLength += 1
                if runLength >= 3 { return runStart }
            } else if !line.trimmingCharacters(in: .whitespaces).isEmpty {
                // A blank line inside a quoted block is still the
                // block; anything else ends the run.
                runLength = 0
                runStart = nil
            }
            // `+ 1` for the separator the split consumed. The last line
            // has none, and stepping past the end is why this is
            // guarded rather than arithmetic.
            lineStart = text.index(lineStart, offsetBy: line.count, limitedBy: text.endIndex)
                .flatMap { text.index($0, offsetBy: 1, limitedBy: text.endIndex) } ?? text.endIndex
        }
        return nil
    }
}
