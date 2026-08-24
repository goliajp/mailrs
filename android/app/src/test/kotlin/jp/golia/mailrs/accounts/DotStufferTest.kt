package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Test

/** Dot-stuffing a message that arrives in pieces. */
class DotStufferTest {
    private fun whole(text: String): String = DotStuffer().feed(text)

    private fun inPieces(text: String, size: Int): String {
        val stuffer = DotStuffer()
        val out = StringBuilder()
        var rest = text
        while (rest.isNotEmpty()) {
            val take = minOf(size, rest.length)
            out.append(stuffer.feed(rest.substring(0, take)))
            rest = rest.substring(take)
        }
        return out.toString()
    }

    /** The ordinary case, and the reason the rule exists. */
    @Test
    fun `a line beginning with a dot gets another`() {
        assertEquals("a\r\n..\r\nb", whole("a\r\n.\r\nb"))
        assertEquals("..hidden", whole(".hidden"))
    }

    /** A dot that is not at a line start is just a dot. */
    @Test
    fun `a dot inside a line is left alone`() {
        assertEquals("see www.example.com", whole("see www.example.com"))
        assertEquals("end.\r\n", whole("end.\r\n"))
    }

    /**
     * **The whole point.** A chunk can end exactly on the line break
     * and the next begin with the dot — a stuffer that forgets treats
     * it as mid-line text, and the message is truncated there while
     * arriving as a complete-looking message that stops halfway.
     */
    @Test
    fun `a dot at the start of the next chunk is still at a line start`() {
        val stuffer = DotStuffer()
        val first = stuffer.feed("hello\r\n")
        val second = stuffer.feed(".\r\nworld")
        assertEquals("hello\r\n..\r\nworld", first + second)
    }

    /**
     * And at **every** split, not just the interesting one. A message
     * cut at each position in turn must come out the same as one cut
     * nowhere.
     */
    @Test
    fun `splitting anywhere gives the same answer as not splitting`() {
        val message = "Subject: x\r\n\r\n.\r\n..\r\nnormal\r\n.dotted\r\nwww.example.com\r\n."
        val reference = whole(message)
        for (size in 1..message.length) {
            assertEquals("split every $size characters", reference, inPieces(message, size))
        }
    }

    /** It agrees with the whole-message version it replaces. */
    @Test
    fun `it matches the one-shot rule`() {
        for (message in listOf(
            ".",
            ".\r\n",
            "a\r\n.b\r\n",
            "\r\n.\r\n",
            "no dots here",
        )) {
            assertEquals(
                message,
                Smtp.dotStuffed(message) + "\r\n",
                whole(message.replace("\r\n", "\n").replace("\n", "\r\n")) + "\r\n",
            )
        }
    }

    /**
     * A lone CR does not start a line. In a message that is a stray
     * byte, and treating it as a break would stuff a dot that is not
     * at a line start.
     */
    @Test
    fun `a lone carriage return does not start a line`() {
        assertEquals("a\r.b", whole("a\r.b"))
    }

    /** An empty piece changes nothing, including the state. */
    @Test
    fun `an empty chunk is harmless`() {
        val stuffer = DotStuffer()
        assertEquals("x\r\n", stuffer.feed("x\r\n"))
        assertEquals("", stuffer.feed(""))
        assertEquals("..y", stuffer.feed(".y"))
    }
}
