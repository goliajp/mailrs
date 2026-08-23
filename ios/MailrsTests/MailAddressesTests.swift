import Testing

@testable import Mailrs

/// Reading an address list out of a header.
@Suite struct MailAddressesTests {
    /// The whole of the difficulty is one character: a display name may
    /// contain a comma, which is why it is quoted. Splitting on every
    /// comma makes two recipients out of one, and one of them is
    /// nonsense.
    @Test func aCommaInsideAQuotedNameIsNotASeparator() {
        #expect(
            MailAddresses.split(#""Lovelace, Ada" <ada@example.com>, bob@example.com"#)
                == [#""Lovelace, Ada" <ada@example.com>"#, "bob@example.com"])
    }

    /// Nor is one inside angle brackets, where an obsolete route lives.
    @Test func aCommaInsideAngleBracketsIsNotASeparator() {
        #expect(
            MailAddresses.split("<@a.example,@b.example:c@d.example>, e@f.example")
                == ["<@a.example,@b.example:c@d.example>", "e@f.example"])
    }

    /// The ordinary case stays ordinary.
    @Test func plainListsSplitOnCommas() {
        #expect(MailAddresses.split("a@b.com,  c@d.com") == ["a@b.com", "c@d.com"])
        #expect(MailAddresses.split("").isEmpty)
        #expect(MailAddresses.split("  ,  ").isEmpty)
    }

    /// For comparing, never for showing: `Ada <a@b>` and `a@b` are the
    /// same person, and a reply-all that does not know it copies
    /// somebody to their own message.
    @Test func theBareAddressIgnoresTheDisplayNameAndTheCase() {
        #expect(MailAddresses.bare("Ada <Ada@Example.COM>") == "ada@example.com")
        #expect(MailAddresses.bare("  ada@example.com ") == "ada@example.com")
        #expect(MailAddresses.bare(#""A, B" <ada@example.com>"#) == "ada@example.com")
    }

    /// A reply-all that copies its own author is the thing everybody
    /// notices, and the thing nobody can undo once it is sent.
    @Test func replyAllNeverCopiesTheSenderOrThePrimaryRecipient() {
        let copies = MailAddresses.replyAll(
            to: "Me <me@example.com>, Ada <ada@example.com>",
            cc: "bob@example.com",
            primary: "Ada <ada@example.com>",
            mine: "me@example.com")
        #expect(copies == ["bob@example.com"])
    }

    /// And nobody appears twice, however they were written.
    @Test func somebodyOnBothToAndCcIsCopiedOnce() {
        let copies = MailAddresses.replyAll(
            to: "Bob <BOB@example.com>",
            cc: "bob@example.com, carol@example.com",
            primary: "ada@example.com",
            mine: "me@example.com")
        #expect(copies == ["Bob <BOB@example.com>", "carol@example.com"])
    }

    /// The order people were written in is the order they stay in.
    @Test func theWrittenOrderIsKept() {
        let copies = MailAddresses.replyAll(
            to: "z@example.com, a@example.com", cc: "m@example.com",
            primary: "x@example.com", mine: "me@example.com")
        #expect(copies == ["z@example.com", "a@example.com", "m@example.com"])
    }
}
