package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationReplyTest {

    private fun message(
        uid: Int,
        sender: String,
        subject: String = "Quarterly review",
        at: Long = 1_754_400_000,
        body: String = "the original",
    ) = Wire.Message(
        uid = uid,
        sender = sender,
        senderTrust = "verified",
        recipients = "me@golia.jp",
        subject = subject,
        flags = 0,
        internalDate = at,
        messageId = "<$uid@golia.jp>",
        textBody = body,
        category = "inbox",
        riskScore = 0,
        riskReason = "",
    )

    @Test
    fun `answers the newest message that is not mine`() {
        val send = NotificationReply.of(
            listOf(
                message(1, "Alice <alice@example.com>", at = 100),
                message(2, "Bob <bob@example.com>", at = 200),
                // Mine, and the newest — answering it would address the
                // reply back at me.
                message(3, "Me <me@golia.jp>", at = 300),
            ),
            myAddress = "me@golia.jp",
            typed = "On it.",
        )!!
        assertEquals(listOf("bob@example.com"), send.to)
        assertEquals("<2@golia.jp>", send.inReplyTo)
    }

    @Test
    fun `keeps the subject the thread already has`() {
        val send = NotificationReply.of(
            listOf(message(1, "alice@example.com", subject = "RE: Quarterly review")),
            myAddress = "me@golia.jp",
            typed = "Yes.",
        )!!
        // Not "Re: RE: Quarterly review".
        assertEquals("RE: Quarterly review", send.subject)
    }

    @Test
    fun `carries what was typed, then the quote`() {
        val send = NotificationReply.of(
            listOf(message(1, "alice@example.com", body = "Are we still on?")),
            myAddress = "me@golia.jp",
            typed = "Yes, 3pm.",
        )!!
        assertTrue(send.body.startsWith("Yes, 3pm."))
        assertTrue(send.body.contains("> Are we still on?"))
    }

    @Test
    fun `will not reply to a thread that is only mine`() {
        assertNull(
            NotificationReply.of(
                listOf(message(1, "me@golia.jp"), message(2, "Me <ME@GOLIA.JP>")),
                myAddress = "me@golia.jp",
                typed = "…",
            ),
        )
    }
}
