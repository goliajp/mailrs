import Testing

@testable import Mailrs

@Suite("Quoted history")
struct QuotedHistoryTests {
    /// The shape this client's own replies produce, and Apple Mail's,
    /// and Gmail's.
    @Test("an attribution line starts the quote")
    func attribution() {
        let split = QuotedHistory.split(
            "Thursday works.\n\nOn Aug 5, 2025, Alice wrote:\n> Can you do Tuesday?")
        #expect(split.body == "Thursday works.")
        // The attribution belongs to the quote: a reply ending on
        // "Alice wrote:" with nothing after it reads like a sentence
        // cut in half.
        #expect(split.quoted?.hasPrefix("On Aug 5, 2025, Alice wrote:") == true)
    }

    /// Japanese clients write the same sentence, and this mailbox is
    /// full of them.
    @Test("the Japanese attribution is recognised too")
    func japanese() {
        let split = QuotedHistory.split("承知しました。\n\n山田さんは、火曜日に書きました:\n> よろしく")
        #expect(split.body == "承知しました。")
        #expect(split.quoted?.contains("よろしく") == true)
    }

    @Test("Outlook and forwards are boundaries")
    func separators() {
        for line in ["-----Original Message-----", "---------- Forwarded message ----------"] {
            let split = QuotedHistory.split("See below.\n\n\(line)\nFrom: someone")
            #expect(split.body == "See below.", "\(line) was not a boundary")
            #expect(split.quoted?.contains(line) == true)
        }
    }

    /// Three, not one: a single `>` line is a quotation inside a
    /// sentence as often as it is history.
    @Test("a run of quoted lines is history; one line is not")
    func quotedRun() {
        let one = QuotedHistory.split("As they put it:\n> a fine idea\nand I agree.")
        #expect(one.quoted == nil, "one quoted line folded away part of the reply")

        let run = QuotedHistory.split("Agreed.\n> line one\n> line two\n> line three")
        #expect(run.body == "Agreed.")
        #expect(run.quoted?.contains("line three") == true)
    }

    /// A bare forward with no note on it is all quote — folding it
    /// leaves an empty card.
    @Test("a message that is nothing but quote is left alone")
    func allQuote() {
        let split = QuotedHistory.split("> line one\n> line two\n> line three")
        #expect(split.quoted == nil)
        #expect(split.body.contains("line one"))
    }

    @Test("ordinary mail is not split")
    func noBoundary() {
        let text = "Just a note, nothing quoted, and the word wrote appears mid-sentence."
        #expect(QuotedHistory.split(text) == QuotedHistory.Split(body: text, quoted: nil))
        #expect(QuotedHistory.split("") == QuotedHistory.Split(body: "", quoted: nil))
    }

    /// Only ever a split: what comes back must be what arrived.
    @Test("nothing is invented and nothing is lost")
    func lossless() {
        let text = "Reply text.\n\nOn Monday, Bob wrote:\n> the original\n> second line\n> third"
        let split = QuotedHistory.split(text)
        let rejoined = split.body + "\n\n" + (split.quoted ?? "")
        #expect(
            rejoined.replacingOccurrences(of: "\n", with: "")
                == text.replacingOccurrences(of: "\n", with: ""))
    }
}
