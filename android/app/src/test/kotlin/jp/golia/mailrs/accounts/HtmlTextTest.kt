package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

/** Readable text out of mail markup. */
class HtmlTextTest {
    @Test
    fun `tags go and text stays`() {
        assertEquals("Hello there.", HtmlText.plain("<p>Hello <b>there</b>.</p>"))
    }

    /** Blocks end lines; everything else does not. */
    @Test
    fun `blocks become line breaks`() {
        assertEquals("one\ntwo", HtmlText.plain("<p>one</p><p>two</p>"))
        assertEquals("first\nsecond", HtmlText.plain("first<br>second"))
        assertEquals("abc", HtmlText.plain("a<span>b</span>c"))
    }

    /**
     * A stylesheet is not the message. Mail from every marketing tool
     * begins with several hundred lines of it.
     */
    @Test
    fun `style and script are not text`() {
        val html = "<html><head><style>p{color:red}</style></head><body>real</body></html>"
        assertEquals("real", HtmlText.plain(html))
        assertEquals("after", HtmlText.plain("<script>var x = 1 < 2;</script>after"))
    }

    /**
     * A self-closed silent element has no closing tag, and waiting for
     * one swallows the rest of the message.
     */
    @Test
    fun `a self closed style does not eat the message`() {
        assertEquals("the message", HtmlText.plain("<style/>the message"))
    }

    @Test
    fun `entities are decoded`() {
        assertEquals(
            "AT&T <tag> \"quoted\"",
            HtmlText.plain("AT&amp;T &lt;tag&gt; &quot;quoted&quot;"),
        )
        assertEquals("AB", HtmlText.plain("&#65;&#x42;"))
        assertEquals("— …", HtmlText.plain("&mdash; &hellip;"))
    }

    /**
     * `&nbsp;` becomes an ordinary space: a non-breaking one is
     * invisible and unbreakable, and a paragraph full of them will not
     * wrap on a phone.
     */
    @Test
    fun `non breaking spaces become ordinary ones`() {
        assertEquals("a b", HtmlText.plain("a&nbsp;b"))
    }

    /** Something that is not an entity is left alone rather than eaten. */
    @Test
    fun `a stray ampersand survives`() {
        assertEquals(
            "rock &amp roll; fish & chips",
            HtmlText.plain("rock &amp roll; fish & chips"),
        )
    }

    /**
     * Generated markup is indented, and every one of those newlines is
     * layout rather than text.
     */
    @Test
    fun `generated whitespace collapses`() {
        val html = "<html>\n  <body>\n    <p>   spaced     out   </p>\n\n\n" +
            "    <p>and again</p>\n  </body>\n</html>"
        assertEquals("spaced out\nand again", HtmlText.plain(html))
    }

    /**
     * A lone CR is a line ending. The iOS side kept one on the end of
     * every message whose last line was not terminated with a full
     * CRLF, because its whitespace set does not include CR; the two
     * clients must not disagree about where a line ends.
     */
    @Test
    fun `a lone carriage return is a line ending`() {
        assertEquals("one\ntwo", HtmlText.plain("<p>one</p>\r<p>two</p>\r"))
        assertFalse(HtmlText.plain("done\r").contains("\r"))
    }

    @Test
    fun `nothing is not a crash`() {
        assertEquals("", HtmlText.plain(""))
        assertEquals("unclosed", HtmlText.plain("<p>unclosed"))
        // A browser shows this as text, and so should a mail body:
        // `<` is only a tag when something could follow it.
        assertEquals("<<>>", HtmlText.plain("<<>>"))
        assertEquals("if a < b and b > c", HtmlText.plain("if a < b and b > c"))
    }
}
