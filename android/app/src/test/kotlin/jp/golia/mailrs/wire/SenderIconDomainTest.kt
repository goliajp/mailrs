package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SenderIconDomainTest {

    @Test
    fun `takes the domain out of a display form`() {
        assertEquals("example.com", SenderIconDomain.of("Alice Smith <alice@example.com>"))
        assertEquals("golia.jp", SenderIconDomain.of("noreply@GOLIA.JP"))
    }

    @Test
    fun `asks for nothing that cannot have an icon`() {
        // No domain at all, or one that no favicon service knows: each
        // of these would be a live four-second cascade answering 204.
        assertNull(SenderIconDomain.of(""))
        assertNull(SenderIconDomain.of("mailer-daemon"))
        assertNull(SenderIconDomain.of("root@localhost"))
        assertNull(SenderIconDomain.of("someone@.example.com"))
        assertNull(SenderIconDomain.of("someone@example..com"))
        // The handler rejects anything that is not hostname-shaped; the
        // same rule here keeps a page of senders from sending a page of
        // requests to be told no.
        assertNull(SenderIconDomain.of("someone@ex ample.com"))
        assertNull(SenderIconDomain.of("someone@exa/mple.com"))
    }
}
