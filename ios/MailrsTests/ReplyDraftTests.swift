import Testing

@testable import Mailrs

/// What a reply starts out as.
@Suite struct ReplyDraftTests {
    private func headers(
        from: String = "Ada <ada@example.com>", replyTo: String = "",
        subject: String = "Lunch", id: String = "<m1@example.com>",
        references: [String] = []
    ) -> MessageHeaders.Parsed {
        var h = MessageHeaders.Parsed()
        h.from = from
        h.replyTo = replyTo
        h.subject = subject
        h.messageId = id
        h.references = references
        return h
    }

    private var me: MailAccount {
        MailAccount.make(address: "me@example.com", displayName: "Me", sort: 0)
    }

    /// That is the entire purpose of the header, and ignoring it sends
    /// replies to a no-reply address.
    @Test func replyToWinsOverFrom() {
        #expect(
            ReplyDraft.recipient(headers(replyTo: "list@example.com")) == "list@example.com")
        #expect(ReplyDraft.recipient(headers()) == "Ada <ada@example.com>")
        // Whitespace is not an address.
        #expect(ReplyDraft.recipient(headers(replyTo: "   ")) == "Ada <ada@example.com>")
    }

    /// One `Re:`, never two — a conversation that has been round a few
    /// times otherwise reads `Re: Re: Re:`, and some clients thread on
    /// the subject.
    @Test func theSubjectGainsOneReAndOnlyOne() {
        #expect(ReplyDraft.subject("Lunch") == "Re: Lunch")
        #expect(ReplyDraft.subject("Re: Lunch") == "Re: Lunch")
        #expect(ReplyDraft.subject("RE: Re: Lunch") == "Re: Lunch")
        #expect(ReplyDraft.subject("re : Lunch") == "Re: Lunch")
    }

    /// The prefixes a phone in Japan or China actually sends.
    @Test func localisedPrefixesCountToo() {
        #expect(ReplyDraft.subject("回复: 午饭") == "Re: 午饭")
        #expect(ReplyDraft.subject("答复: Re: 午饭") == "Re: 午饭")
    }

    /// A subject that is only a prefix, and one that is nothing.
    @Test func anEmptySubjectStillBecomesAReply() {
        #expect(ReplyDraft.subject("") == "Re:")
        #expect(ReplyDraft.subject("Re:") == "Re: ")
    }

    /// Threading is carried, or the reply starts a new conversation in
    /// every client that reads it.
    @Test func theConversationIsCarried() {
        let draft = ReplyDraft.make(
            to: headers(references: ["<m0@example.com>"]), from: me)
        #expect(draft.inReplyTo == "<m1@example.com>")
        #expect(draft.references == ["<m0@example.com>", "<m1@example.com>"])
    }

    /// A message already in its own References must not be listed
    /// twice.
    @Test func theParentIsNotRepeated() {
        let draft = ReplyDraft.make(
            to: headers(references: ["<m0@example.com>", "<m1@example.com>"]), from: me)
        #expect(draft.references == ["<m0@example.com>", "<m1@example.com>"])
    }

    /// Quoting, with an attribution line — and nothing at all when
    /// there is nothing to quote, because an attribution above an
    /// empty quote reads as a message that failed to load.
    @Test func theOriginalIsQuoted() {
        let out = ReplyDraft.quoted("one\n\ntwo", from: headers())
        #expect(out.contains("Ada <ada@example.com> wrote:"))
        #expect(out.contains("> one"))
        #expect(out.contains("\n>\n"), "an empty quoted line kept a trailing space")
        #expect(!out.contains("> two\n> "))

        #expect(ReplyDraft.quoted("", from: headers()) == "")
        #expect(ReplyDraft.quoted("   \n\n ", from: headers()) == "")
    }

    /// The reply is from the account it is sent from, with that
    /// account's name — not the name of whoever is being replied to.
    @Test func theReplyIsFromTheAccount() {
        let draft = ReplyDraft.make(to: headers(), from: me)
        #expect(draft.from == "me@example.com")
        #expect(draft.fromName == "Me")
        #expect(draft.to == ["Ada <ada@example.com>"])
    }
}
