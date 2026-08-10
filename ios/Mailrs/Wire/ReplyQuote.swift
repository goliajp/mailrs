import Foundation

/// What a reply carries of the message it answers.
///
/// A reply sent from this app arrived with the reader's sentence and
/// nothing else: no attribution line, no original text. On a thread of
/// any length the recipient is left working out which of five questions
/// "yes, Thursday" answers. The web client has always quoted; the phone
/// did not, and the phone is where short replies get written.
///
/// The attribution line is the one every client writes and every client
/// recognises — `email-split.ts` on the web finds `On …, … wrote:` to
/// fold quoted history away again, and Apple Mail and Gmail both parse
/// it. Matching that wording is what lets the other end collapse this.
enum ReplyQuote {
    /// The body to send: what was typed, then the original beneath it.
    ///
    /// `>`-prefixed, which is the plain-text convention RFC 3676 builds
    /// on and what the quote-folding in every reader keys on. An empty
    /// original yields the reply unchanged rather than a header with
    /// nothing under it.
    static func body(
        typed: String, from: String, date: String, original: String, limit: Int = 10_000
    ) -> String {
        let trimmed = original.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return typed }
        let quoted = quote(trimmed, limit: limit)
        return "\(typed)\n\n\(attribution(from: from, date: date))\n\(quoted)"
    }

    /// `On <date>, <sender> wrote:` — or the shorter form when there is
    /// no date to give, rather than the word "on" followed by nothing.
    static func attribution(from: String, date: String) -> String {
        let who = from.trimmingCharacters(in: .whitespacesAndNewlines)
        let when = date.trimmingCharacters(in: .whitespacesAndNewlines)
        if who.isEmpty { return "Quoted message:" }
        if when.isEmpty { return "\(who) wrote:" }
        return "On \(when), \(who) wrote:"
    }

    /// Every line prefixed, and the whole thing bounded.
    ///
    /// A 20KB digest quoted in full is a reply nobody can read on a
    /// phone and a message body several times the size of the sentence
    /// it carries. Cut on a line boundary so the last quoted line is a
    /// line.
    static func quote(_ text: String, limit: Int) -> String {
        var kept: [String] = []
        var used = 0
        var truncated = false
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            if used + line.count > limit {
                truncated = true
                break
            }
            used += line.count + 1
            kept.append("> " + line.trimmingCharacters(in: CharacterSet(charactersIn: "\r")))
        }
        if truncated { kept.append("> […]") }
        return kept.joined(separator: "\n")
    }
}
