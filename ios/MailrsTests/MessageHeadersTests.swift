import Testing

@testable import Mailrs

/// The few headers a list row needs.
@Suite struct MessageHeadersTests {
    private let message = """
        From: Alice Smith <alice@example.com>\r
        To: bob@golia.jp\r
        Subject: Quarterly report\r
        Message-ID: <m1@example.com>\r
        Date: Tue, 5 Aug 2026 09:00:00 +0900\r
        \r
        Subject: this is body text, not a header\r
        """

    @Test func theRowsFieldsAreRead() {
        let p = MessageHeaders.parse(message)
        #expect(p.from == "Alice Smith <alice@example.com>")
        #expect(p.subject == "Quarterly report")
        #expect(p.messageId == "<m1@example.com>")
    }

    /// A body may contain anything, including lines that look exactly
    /// like headers. A parser that keeps going reads them.
    @Test func theBodyIsNotReadAsHeaders() {
        #expect(MessageHeaders.parse(message).subject == "Quarterly report")
    }

    /// **Folding is the trap.** A long Subject continues on the next
    /// line, and a parser that reads lines gets half of it.
    @Test func aFoldedSubjectIsWhole() {
        let raw = "Subject: Quarterly report and\r\n the follow-up notes\r\n\r\nbody"
        #expect(MessageHeaders.parse(raw).subject == "Quarterly report and the follow-up notes")
    }

    /// And the continuation must not become a header of its own.
    @Test func aContinuationIsNotItsOwnHeader() {
        let raw = "Subject: one\r\n Date: not-a-date\r\nDate: Tue, 5 Aug 2026 09:00:00 +0900\r\n\r\n"
        let p = MessageHeaders.parse(raw)
        #expect(p.subject == "one Date: not-a-date")
        #expect(p.date == "Tue, 5 Aug 2026 09:00:00 +0900")
    }

    /// A repeated header keeps the first: a message with two Subjects
    /// is malformed, and picking the last lets somebody append one.
    @Test func aRepeatedHeaderKeepsTheFirst() {
        let raw = "Subject: real\r\nSubject: injected\r\n\r\n"
        #expect(MessageHeaders.parse(raw).subject == "real")
    }

    @Test func aDisplayNameIsPreferredToAnAddress() {
        #expect(MessageHeaders.senderName("Alice Smith <alice@example.com>") == "Alice Smith")
        #expect(MessageHeaders.senderName("alice@example.com") == "alice@example.com")
        #expect(MessageHeaders.senderName("<alice@example.com>") == "alice@example.com")
    }

    /// A name with a comma is quoted, and the quotes are syntax.
    @Test func quotesComeOffAName() {
        #expect(MessageHeaders.senderName(#""Smith, Alice" <a@x.com>"#) == "Smith, Alice")
    }
}

/// The subject and the sender arrive decoded.
///
/// Every reader of a subject wants the text; one that forgets to
/// decode shows `=?UTF-8?B?` to somebody. Doing it at the door means
/// no reader can forget.
@Suite struct DecodedHeadersTests {
    @Test func aJapaneseSubjectIsReadable() {
        let raw = "Subject: =?UTF-8?B?5Lya6K2w44Gu5Lu2?=\r\nFrom: a@x.jp\r\n\r\n"
        #expect(MessageHeaders.parse(raw).subject == "会議の件")
    }

    @Test func anEncodedDisplayNameIsReadable() {
        let raw = "From: =?UTF-8?B?55Sw5Lit?= <tanaka@x.jp>\r\n\r\n"
        let p = MessageHeaders.parse(raw)
        #expect(MessageHeaders.senderName(p.from) == "田中")
    }

    /// A folded, encoded subject is both traps at once: unfold first,
    /// then decode, or the two halves decode separately and the gap
    /// between them survives as a space.
    @Test func aFoldedEncodedSubjectIsWholeAndReadable() {
        let raw = "Subject: =?UTF-8?B?5Lya6K2w?=\r\n =?UTF-8?B?44Gu5Lu2?=\r\n\r\n"
        #expect(MessageHeaders.parse(raw).subject == "会議の件")
    }
}
