package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Which address a message leaves by.
 *
 * The rule is the same on all three clients and each has its own copy
 * of this test, because getting it wrong is invisible: the message
 * sends, and lands in the conversation as a stranger.
 */
class ReplyFromTest {
    private fun account(id: String, email: String, state: String = "ok", name: String = "") =
        ExternalAccount(id = id, email = email, state = state, displayName = name)

    @Test
    fun `this server's own address comes first`() {
        val out = fromAddresses("me@golia.jp", listOf(account("a1", "me@gmail.com")))
        assertEquals("me@golia.jp", out.first().address)
        assertEquals("", out.first().accountId)
    }

    // Choosing it would produce a message that cannot be sent, and
    // offering a choice that fails is worse than not offering it.
    @Test
    fun `an account whose credential was refused is not offered`() {
        val out = fromAddresses("me@golia.jp", listOf(account("a1", "x@gmail.com", "needs_auth")))
        assertEquals(1, out.size)
    }

    @Test
    fun `a named account shows its name beside the address`() {
        val out = fromAddresses("me@golia.jp", listOf(account("a1", "x@gmail.com", name = "Work")))
        assertEquals("Work · x@gmail.com", out[1].label)
    }

    @Test
    fun `a name equal to the address is not repeated`() {
        val out = fromAddresses("me@golia.jp", listOf(account("a1", "x@gmail.com", name = "x@gmail.com")))
        assertEquals("x@gmail.com", out[1].label)
    }

    // A reply to mail that arrived at a connected Gmail goes out
    // through that Gmail.
    @Test
    fun `a reply follows the account the mail arrived at`() {
        val addresses = fromAddresses("me@golia.jp", listOf(account("a1", "x@gmail.com")))
        assertEquals("x@gmail.com", replyFromFor("a1", addresses))
    }

    @Test
    fun `mail that arrived here leaves from here`() {
        val addresses = fromAddresses("me@golia.jp", listOf(account("a1", "x@gmail.com")))
        assertEquals("me@golia.jp", replyFromFor("", addresses))
        assertEquals("me@golia.jp", replyFromFor(null, addresses))
    }

    // Replying from somewhere beats a composer that will not send.
    @Test
    fun `an account that is gone falls back rather than refusing`() {
        val addresses = fromAddresses("me@golia.jp", listOf(account("a1", "x@gmail.com")))
        assertEquals("me@golia.jp", replyFromFor("deleted", addresses))
    }
}

/**
 * The second line of an account row.
 *
 * An account with no name of its own falls back to the address on the
 * first line, so repeating it underneath says nothing — and the row
 * carried the same text twice, which is what the instrumentation test
 * reported as "found 2 nodes".
 */
class AccountSubtitleTest {
    @Test
    fun `a named account shows its address underneath`() {
        assertEquals("x@gmail.com", accountSubtitle("Work", "x@gmail.com"))
    }

    @Test
    fun `an unnamed account does not repeat itself`() {
        assertNull(accountSubtitle("", "x@gmail.com"))
    }

    @Test
    fun `a name equal to the address does not repeat itself either`() {
        assertNull(accountSubtitle("x@gmail.com", "x@gmail.com"))
    }
}
