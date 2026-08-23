package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The decisions in sending that need no server. */
class AccountSenderTest {
    private fun account(address: String) =
        MailAccount.make(address, "", 0).copy(
            smtpHost = "smtp.example.com",
            smtpPort = 587,
        )

    /**
     * A Message-ID pointing at a domain that has nothing to do with the
     * sender is one of the things spam filters count.
     */
    @Test
    fun `the message identity uses the senders own domain`() {
        assertEquals(
            "abc-123@example.com",
            AccountSender.identity(account("me@example.com"), "ABC-123"),
        )
    }

    /**
     * A HELO naming somebody's phone is refused by a fair number of
     * servers and greylisted by more.
     */
    @Test
    fun `the greeting names a domain`() {
        assertEquals("example.com", AccountSender.helo(account("me@example.com")))
        assertEquals("localhost", AccountSender.helo(account("nonsense")))
    }

    /**
     * 4xx is the moment's fault and 5xx is the message's. Somebody told
     * "try again" about a permanent refusal will try forever.
     */
    @Test
    fun `a temporary refusal says try again and a permanent one does not`() {
        val temporary = AccountSender.explain(
            SmtpSession.Failure.Rejected(451, "busy", false),
        )
        assertTrue(temporary, temporary.contains("try again"))

        val permanent = AccountSender.explain(
            SmtpSession.Failure.Rejected(550, "no such user", true),
        )
        assertFalse(permanent, permanent.contains("try again"))
        assertTrue(permanent, permanent.contains("no such user"))
    }

    /**
     * The one refusal a person can actually do something about gets its
     * own sentence.
     */
    @Test
    fun `a refused sign in says so`() {
        val out = AccountSender.explain(SmtpSession.Failure.Rejected(535, "auth failed", true))
        assertTrue(out, out.contains("sign-in"))
    }

    /**
     * Every failure produces a sentence — an empty message is a screen
     * that says nothing went wrong while nothing was sent.
     */
    @Test
    fun `every failure says something`() {
        val all = listOf(
            SmtpSession.Failure.Unreachable("nw error 61"),
            SmtpSession.Failure.Refused("bad greeting"),
            SmtpSession.Failure.Closed(),
            SmtpSession.Failure.Rejected(421, "", false),
        )
        for (failure in all) assertTrue(AccountSender.explain(failure).isNotEmpty())
    }
}
