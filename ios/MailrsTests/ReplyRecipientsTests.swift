import Testing

@testable import Mailrs

/// The web's reply recipient rules (`thread-view.tsx`), kept in step.
struct ReplyRecipientsTests {
    @Test func replyGoesToTheSenderAlone() {
        #expect(ReplyRecipients.reply(toSender: "Alice Smith <alice@example.com>")
            == ["alice@example.com"])
    }

    /// Sender plus the To line, minus me — a reply must not arrive
    /// addressed back at the person sending it.
    @Test func replyAllIsEveryoneExceptMe() {
        let all = ReplyRecipients.replyAll(
            sender: "Alice <alice@example.com>",
            recipients: "me@golia.jp, Bob <bob@example.com>",
            myAddress: "me@golia.jp"
        )
        #expect(all == ["alice@example.com", "bob@example.com"])
    }

    @Test func replyAllDeduplicatesAndNormalises() {
        let all = ReplyRecipients.replyAll(
            sender: "alice@example.com",
            recipients: "Alice <ALICE@Example.com>; bob@example.com",
            myAddress: "me@golia.jp"
        )
        #expect(all == ["alice@example.com", "bob@example.com"])
    }

    /// A conversation with only me and the sender: reply-all equals
    /// reply, not an empty To line.
    @Test func replyAllToASoloSenderIsJustTheSender() {
        let all = ReplyRecipients.replyAll(
            sender: "alice@example.com",
            recipients: "me@golia.jp",
            myAddress: "me@golia.jp"
        )
        #expect(all == ["alice@example.com"])
    }

    @Test func subjectsGainOnePrefixOnly() {
        #expect(ReplyRecipients.subject("Hello", forwarding: false) == "Re: Hello")
        #expect(ReplyRecipients.subject("Re: Hello", forwarding: false) == "Re: Hello")
        #expect(ReplyRecipients.subject("RE: Hello", forwarding: false) == "RE: Hello")
        #expect(ReplyRecipients.subject("Hello", forwarding: true) == "Fwd: Hello")
        #expect(ReplyRecipients.subject("Fwd: Hello", forwarding: true) == "Fwd: Hello")
        #expect(ReplyRecipients.subject("", forwarding: false) == "Re:")
    }
}
