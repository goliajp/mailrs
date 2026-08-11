import Testing

@testable import Mailrs

@Suite("Copying and sharing one message")
struct MessageActionsTests {
    private func message(text: String? = nil, html: String? = nil, subject: String = "Subject")
        -> Wire.Message
    {
        Wire.Message(
            uid: 1, sender: "a@b.com", senderTrust: "", recipients: "c@d.com",
            subject: subject, internalDate: 0, messageId: "<m@x>",
            textBody: text, htmlBody: html, attachments: [], unsubscribe: nil)
    }

    @Test("the plain part is what gets copied when there is one")
    func prefersPlain() {
        let m = message(text: "Hello there", html: "<p>Hello <b>there</b></p>")
        #expect(MessageActions.plainText(m) == "Hello there")
    }

    /// Pasting markup into a chat window is not what anyone meant by
    /// "copy".
    @Test("html is stripped when that is all there is")
    func stripsHtml() {
        let m = message(html: "<p>Hello <b>there</b></p>")
        #expect(MessageActions.plainText(m) == "Hello there")
    }

    @Test("script and style go whole, not just their tags")
    func dropsCode() {
        let stripped = MessageActions.stripped(
            "<style>.a{color:red}</style><p>Read me</p><script>alert(1)</script>")
        #expect(stripped == "Read me")
    }

    @Test("block boundaries become line breaks")
    func blocksBreak() {
        let stripped = MessageActions.stripped("<p>One</p><p>Two</p>")
        #expect(stripped == "One\nTwo")
    }

    /// A table-based newsletter leaves runs of blank lines behind; one
    /// is what a person would have typed.
    @Test("runs of blank lines collapse to one")
    func collapsesBlanks() {
        // Empty `<p></p>` pairs leave bare newlines with nothing
        // between them — the first rule asked for blank *lines* and
        // there were none, so a newsletter arrived with a hole in it.
        #expect(MessageActions.stripped("<p>One</p><p></p><p></p><p>Two</p>") == "One\n\nTwo")
        #expect(MessageActions.stripped("<p>One</p>\n \n\n \n<p>Two</p>") == "One\n\nTwo")
    }

    @Test("entities come back as characters")
    func entities() {
        #expect(MessageActions.stripped("<p>Tom &amp; Jerry &lt;3</p>") == "Tom & Jerry <3")
    }

    /// A body alone arrives at the other end with no idea what it is.
    @Test("sharing carries the subject")
    func shareCarriesSubject() {
        let m = message(text: "Body here", subject: "The subject")
        #expect(MessageActions.shareable(m) == "The subject\n\nBody here")
    }

    /// Nine bodies in a 900-message sample carry nothing at all; an
    /// empty answer is honest.
    @Test("a message with no body copies nothing rather than markup")
    func emptyBody() {
        #expect(MessageActions.plainText(message()) == "")
        #expect(MessageActions.shareable(message(subject: "Only a subject")) == "Only a subject")
    }
}
