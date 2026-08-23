package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ManualEndpointsTest {
    private fun e(host: String, port: String, proto: String = "imap") =
        ManualEndpoint(host = host, port = port, proto = proto)

    @Test
    fun `both endpoints go out when both are complete`() {
        val out = wireEndpoints(e("imap.x.jp", "993"), e("smtp.x.jp", "465", "smtp"))
        assertEquals(WireEndpoint("imap.x.jp", 993, "imap", "implicit"), out?.first)
        assertEquals(WireEndpoint("smtp.x.jp", 465, "smtp", "implicit"), out?.second)
    }

    // An empty box must not become a real port. Surrounding spaces
    // are trimmed first — somebody pasting " 993 " meant 993 — but
    // anything that is not a digit after that is refused, so "+993"
    // does not arrive as a port nobody typed.
    @Test
    fun `an empty port is refused rather than sent`() {
        assertNull(wireEndpoints(e("imap.x.jp", ""), e("smtp.x.jp", "465", "smtp")))
    }

    @Test
    fun `a half-filled pair never leaves the phone`() {
        assertNull(wireEndpoints(e("", "993"), e("smtp.x.jp", "465", "smtp")))
        assertNull(wireEndpoints(e("imap.x.jp", "993"), e("", "465", "smtp")))
    }

    @Test
    fun `a port outside the range is refused`() {
        for (p in listOf("0", "65536", "99999")) {
            assertNull("port $p was accepted", wireEndpoint(e("h", p)))
        }
    }

    @Test
    fun `only digits count as a port`() {
        for (p in listOf("+993", "9 9", "99.5", "abc", "1e3")) {
            assertNull("port $p was accepted", wireEndpoint(e("h", p)))
        }
    }

    @Test
    fun `spaces around a port are somebody's paste, not a mistake`() {
        assertEquals(993, wireEndpoint(e("h", " 993 "))?.port)
    }

    @Test
    fun `the protocol and the encryption survive`() {
        val out = wireEndpoint(ManualEndpoint("pop.x.jp", "110", "pop3", "starttls"))
        assertEquals("pop3", out?.protocol)
        assertEquals("starttls", out?.tls)
    }
}
