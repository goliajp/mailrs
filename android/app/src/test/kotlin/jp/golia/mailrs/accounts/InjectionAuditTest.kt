package jp.golia.mailrs.accounts

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Base64

/**
 * Does a header from a stranger reach a command line?
 *
 * An audit, written to find out rather than to confirm. A reply's
 * recipient comes from the `Reply-To:` of a message somebody else
 * wrote, and an encoded word decodes to **anything at all** — so if a
 * CRLF survives that trip, replying to a hostile message injects SMTP
 * commands into this client's session.
 */
class InjectionAuditTest {
    private fun encoded(text: String): String =
        "=?utf-8?B?" + Base64.getEncoder().encodeToString(text.toByteArray()) + "?="

    /** What a decoded `Reply-To` actually contains. */
    @Test
    fun a_decoded_reply_to_cannot_carry_a_line_break() {
        val nasty = encoded("victim@example.com>\r\nRCPT TO:<attacker@evil.example")
        val headers = MessageHeaders.parse("Reply-To: $nasty\r\nFrom: a@b\r\n\r\nbody")
        val recipient = ReplyDraft.recipient(headers)
        assertFalse(
            "a line break reached the recipient: <$recipient>",
            recipient.contains('\r') || recipient.contains('\n'),
        )
    }

    /** And the same value as it would reach a `To:` header. */
    @Test
    fun a_decoded_subject_cannot_carry_a_line_break() {
        val nasty = encoded("Hi\r\nBcc: attacker@evil.example")
        val headers = MessageHeaders.parse("Subject: $nasty\r\nFrom: a@b\r\n\r\nbody")
        assertFalse(
            "a line break reached the subject: <${headers.subject}>",
            headers.subject.contains('\r') || headers.subject.contains('\n'),
        )
    }

    /** A built message must not gain a header from either. */
    @Test
    fun a_built_reply_has_no_injected_header() {
        val nasty = encoded("victim@example.com>\r\nBcc: attacker@evil.example\r\nX: <")
        val headers = MessageHeaders.parse("Reply-To: $nasty\r\nFrom: a@b\r\n\r\nbody")
        val me = MailAccount.make("me@example.com", "Me", 0)
        val draft = ReplyDraft.make(headers, me)
        val message = OutgoingMessage.text(
            draft, "x@example.com", 1_756_000_000L, java.util.TimeZone.getTimeZone("UTC"),
        )
        val injected = message.split("\r\n").any {
            it.startsWith("Bcc:", ignoreCase = true) || it.startsWith("X:", ignoreCase = true)
        }
        assertFalse("a header was injected through Reply-To", injected)
    }

    /**
     * **The envelope is a command line.** An address with a control
     * character in it does not become a worse address — it becomes
     * another SMTP command, and the message goes somewhere the sender
     * never typed.
     */
    @Test
    fun an_address_with_a_line_break_never_reaches_the_envelope() {
        val draft = OutgoingMessage.Draft(
            from = "me@example.com",
            to = listOf("you@example.com", "victim@x.example>\r\nRCPT TO:<attacker@evil.example"),
            cc = listOf("ok@example.com"),
        )
        val envelope = OutgoingMessage.envelope(draft, listOf("bcc\r\nDATA@x.example"))
        for (address in envelope) {
            assertFalse(
                "a control character reached the envelope: <$address>",
                address.any { it.code < 0x20 || it.code == 0x7F },
            )
        }
        // And the good ones are still there — a rule that drops
        // everything would pass the assertion above and send nothing.
        assertTrue(envelope.contains("you@example.com"))
        assertTrue(envelope.contains("ok@example.com"))
    }
}
