package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ReplyFromTest {
    private fun acct(id: String, email: String, state: String = "ok") =
        ExternalAccount(id = id, email = email, displayName = "", state = state)

    private val own = "me@golia.jp"
    private val all = fromAddresses(own, listOf(acct("ext_g", "me@gmail.com"), acct("ext_q", "me@qq.com")))

    /**
     * The one that breaks threads: a reply to mail that arrived at a
     * connected Gmail has to go out through that Gmail.
     */
    @Test
    fun `a reply follows the account the mail arrived at`() {
        assertEquals("me@gmail.com", replyFromFor("ext_g", all))
        assertEquals("me@qq.com", replyFromFor("ext_q", all))
    }

    @Test
    fun `mail that arrived here is answered from here`() {
        assertEquals(own, replyFromFor("", all))
        assertEquals(own, replyFromFor(null, all))
    }

    /** Replying from somewhere beats a composer that will not send. */
    @Test
    fun `an account that is gone falls back`() {
        assertEquals(own, replyFromFor("ext_gone", all))
    }

    /**
     * Offering a choice that cannot send is worse than not offering
     * it — the message would fail at the provider, and some of them
     * count the attempts.
     */
    @Test
    fun `an account whose password was refused is not offered`() {
        val list = fromAddresses(own, listOf(acct("ext_x", "me@x.com", state = "needs_auth")))
        assertEquals(1, list.size)
        assertTrue(list.none { it.accountId == "ext_x" })
    }

    @Test
    fun `nothing to send from is empty rather than a crash`() {
        assertEquals("", replyFromFor("ext_g", emptyList()))
    }
}
