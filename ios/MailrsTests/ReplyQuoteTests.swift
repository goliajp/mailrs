import Foundation
import Testing

@testable import Mailrs

@Suite("Reply quoting")
struct ReplyQuoteTests {
    @Test("the reply carries what it answers")
    func quotesTheOriginal() {
        let out = ReplyQuote.body(
            typed: "Thursday works.", from: "Alice", date: "Aug 5, 2025",
            original: "Can you do Tuesday\nor Thursday?")
        #expect(out == """
        Thursday works.

        On Aug 5, 2025, Alice wrote:
        > Can you do Tuesday
        > or Thursday?
        """)
    }

    /// The wording every client writes and every client folds away —
    /// the web's own `email-split.ts` looks for exactly this.
    @Test("the attribution degrades rather than reading oddly")
    func attribution() {
        #expect(ReplyQuote.attribution(from: "Alice", date: "Aug 5")
            == "On Aug 5, Alice wrote:")
        #expect(ReplyQuote.attribution(from: "Alice", date: "") == "Alice wrote:")
        #expect(ReplyQuote.attribution(from: "", date: "Aug 5") == "Quoted message:")
    }

    @Test("nothing to quote leaves the reply alone")
    func emptyOriginal() {
        #expect(ReplyQuote.body(typed: "ok", from: "A", date: "d", original: "") == "ok")
        #expect(ReplyQuote.body(typed: "ok", from: "A", date: "d", original: "  \n ") == "ok")
    }

    /// A 20KB digest quoted whole is a body several times the size of
    /// the sentence it carries, on the device least able to show it.
    @Test("a long original is cut on a line boundary")
    func bounded() {
        let long = (1...500).map { "line \($0) of the digest" }.joined(separator: "\n")
        let out = ReplyQuote.body(typed: "ok", from: "A", date: "d", original: long, limit: 200)
        #expect(out.count < 400)
        #expect(out.hasSuffix("> […]"))
        for line in out.split(separator: "\n").dropFirst(3) {
            #expect(line.hasPrefix("> "), "unprefixed: \(line)")
        }
    }

    @Test("CRLF does not leave a stray return in the quote")
    func crlf() {
        let out = ReplyQuote.body(
            typed: "ok", from: "A", date: "d", original: "one\r\ntwo\r\n")
        #expect(!out.contains("\r"))
    }
}
