import Testing

@testable import Mailrs

struct ThreadCollapseTests {
    /// The newest message is the reason you came; the rest are context.
    @Test func onlyTheLastStartsExpanded() {
        #expect(ThreadCollapse.isExpanded(uid: 2, lastUid: 2, toggled: []))
        #expect(!ThreadCollapse.isExpanded(uid: 1, lastUid: 2, toggled: []))
    }

    /// Toggling works both directions: an older message opens, the
    /// newest can be folded away.
    @Test func togglingInvertsEitherDirection() {
        #expect(ThreadCollapse.isExpanded(uid: 1, lastUid: 2, toggled: [1]))
        #expect(!ThreadCollapse.isExpanded(uid: 2, lastUid: 2, toggled: [2]))
    }

    @Test func aSingleMessageThreadIsExpanded() {
        #expect(ThreadCollapse.isExpanded(uid: 7, lastUid: 7, toggled: []))
    }

    @Test func snippetsFoldWhitespaceAndBound() {
        #expect(ThreadCollapse.snippet("  a\n\n  b\tc  ") == "a b c")
        #expect(ThreadCollapse.snippet(nil) == "")
        let long = String(repeating: "x", count: 200)
        let cut = ThreadCollapse.snippet(long)
        #expect(cut.count == 81 && cut.hasSuffix("…"))
    }

    private func message(text: String?, html: String?) -> Wire.Message {
        Wire.Message(
            uid: 1, sender: "a@b.example", senderTrust: "", recipients: "me", subject: "s",
            internalDate: 1, messageId: "<1>", textBody: text, htmlBody: html,
            attachments: [], unsubscribe: nil)
    }

    /// An HTML-only message folds to its words, not to nothing.
    @Test func anHtmlOnlyMessageFoldsToItsText() {
        let html = "<html><head><style>p{color:red}</style></head><body>"
            + "<script>var x = 1</script><div>Thanks,\n  works</div><div>for me.</div></body></html>"
        #expect(ThreadCollapse.snippet(for: message(text: nil, html: html)) == "Thanks, works for me.")
        #expect(ThreadCollapse.snippet(for: message(text: "", html: "<p>hello</p>")) == "hello")
    }

    /// The earlier mail a reply quotes is not what this message says.
    @Test func theQuotedEarlierMailIsLeftOut() {
        let gmail = "<div>Sounds good</div><div class=\"gmail_quote\"><div class=\"gmail_attr\">On Mon, A wrote:</div>"
            + "<blockquote class=\"gmail_quote\"><div>old <blockquote>older</blockquote> still old</div></blockquote></div>"
        #expect(ThreadCollapse.snippet(for: message(text: nil, html: gmail)) == "Sounds good")
        let outlook = "<div><p>See you then</p></div><div id=\"appendonsend\"></div><hr>"
            + "<div id=\"divRplyFwdMsg\"><b>From:</b> A</div><div>the old message</div>"
        #expect(ThreadCollapse.snippet(for: message(text: nil, html: outlook)) == "See you then")
    }

    /// Cut to the same length as a plain-text snippet.
    @Test func anHtmlSnippetIsBoundLikeAPlainOne() {
        let long = String(repeating: "x", count: 200)
        #expect(ThreadCollapse.snippet(for: message(text: nil, html: "<p>\(long)</p>")) == ThreadCollapse.snippet(long))
        #expect(ThreadCollapse.snippet(for: message(text: "plain wins", html: "<p>html</p>")) == "plain wins")
    }
}
