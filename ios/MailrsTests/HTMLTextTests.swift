import Testing

@testable import Mailrs

/// Readable text out of mail markup.
@Suite struct HTMLTextTests {
    @Test func tagsGoAndTextStays() {
        #expect(HTMLText.plain("<p>Hello <b>there</b>.</p>") == "Hello there.")
    }

    /// Blocks end lines; everything else does not.
    @Test func blocksBecomeLineBreaks() {
        #expect(HTMLText.plain("<p>one</p><p>two</p>") == "one\ntwo")
        #expect(HTMLText.plain("first<br>second") == "first\nsecond")
        #expect(HTMLText.plain("a<span>b</span>c") == "abc")
    }

    /// A stylesheet is not the message. Mail from every marketing tool
    /// begins with several hundred lines of it.
    @Test func styleAndScriptAreNotText() {
        let html = "<html><head><style>p{color:red}</style></head><body>real</body></html>"
        #expect(HTMLText.plain(html) == "real")
        #expect(HTMLText.plain("<script>var x = 1 < 2;</script>after") == "after")
    }

    /// A self-closed silent element has no closing tag, and waiting for
    /// one swallows the rest of the message.
    @Test func aSelfClosedStyleDoesNotEatTheMessage() {
        #expect(HTMLText.plain("<style/>the message") == "the message")
    }

    @Test func entitiesAreDecoded() {
        #expect(HTMLText.plain("AT&amp;T &lt;tag&gt; &quot;quoted&quot;") == "AT&T <tag> \"quoted\"")
        #expect(HTMLText.plain("&#65;&#x42;") == "AB")
        #expect(HTMLText.plain("&mdash; &hellip;") == "— …")
    }

    /// `&nbsp;` becomes an ordinary space: a non-breaking one is
    /// invisible and unbreakable, and a paragraph full of them will not
    /// wrap on a phone.
    @Test func nonBreakingSpacesBecomeOrdinaryOnes() {
        #expect(HTMLText.plain("a&nbsp;b") == "a b")
    }

    /// Something that is not an entity is left alone rather than eaten.
    @Test func aStrayAmpersandSurvives() {
        #expect(HTMLText.plain("rock &amp roll; fish & chips") == "rock &amp roll; fish & chips")
    }

    /// Generated markup is indented, and every one of those newlines is
    /// layout rather than text.
    @Test func generatedWhitespaceCollapses() {
        let html = """
            <html>
              <body>
                <p>   spaced     out   </p>


                <p>and again</p>
              </body>
            </html>
            """
        #expect(HTMLText.plain(html) == "spaced out\nand again")
    }

    /// A lone CR is a line ending. Swift's `.whitespaces` is space and
    /// tab only — CR is in `.newlines` — so trimming with the first
    /// leaves an invisible character on the end of every message whose
    /// last line was not terminated with a full CRLF.
    @Test func aLoneCarriageReturnIsALineEnding() {
        #expect(HTMLText.plain("<p>one</p>\r<p>two</p>\r") == "one\ntwo")
        #expect(!HTMLText.plain("done\r").contains("\r"))
    }

    @Test func nothingIsNotACrash() {
        #expect(HTMLText.plain("") == "")
        #expect(HTMLText.plain("<p>unclosed") == "unclosed")
        // A browser shows this as text, and so should a mail body:
        // `<` is only a tag when something could follow it.
        #expect(HTMLText.plain("<<>>") == "<<>>")
        #expect(HTMLText.plain("if a < b and b > c") == "if a < b and b > c")
    }
}
