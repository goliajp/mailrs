package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Test


/** What a row shows when a header is missing. */
class MailboxRowDisplayTest {
    private fun row(sender: String, subject: String) = MailboxRow(
        accountId = "a", uid = 1L, folder = "INBOX", seen = false,
        sender = sender, subject = subject, date = null, messageId = "m",
    )

    /**
     * A blank line where a name goes reads as a rendering fault; a
     * parenthesised absence reads as an absence. Whitespace counts as
     * missing — a header of two spaces is not a sender.
     */
    @Test
    fun `an absent sender says so`() {
        assertEquals("(no sender)", row("", "s").displaySender)
        assertEquals("(no sender)", row("   ", "s").displaySender)
        assertEquals("Ada", row("Ada", "s").displaySender)
    }

    /** The same for a subject, which is missing far more often. */
    @Test
    fun `an absent subject says so`() {
        assertEquals("(no subject)", row("a", "").displaySubject)
        assertEquals("(no subject)", row("a", " \t ").displaySubject)
    }

    /** Real text is never altered — no trimming of what somebody wrote. */
    @Test
    fun `text that exists is left alone`() {
        assertEquals("Re: lunch", row("a", "Re: lunch").displaySubject)
    }
}
