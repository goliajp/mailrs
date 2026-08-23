package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** How large a message this client can send. */
class OutgoingLimitsTest {
    private fun draft(vararg sizes: Int) = OutgoingMessage.Draft(
        from = "me@example.com",
        to = listOf("you@example.com"),
        body = "words",
        attachments = sizes.mapIndexed { i, size ->
            OutgoingMessage.Attachment("f$i.bin", "application/octet-stream", ByteArray(size))
        },
    )

    /** No attachment is never too large. */
    @Test
    fun `a plain message always passes`() {
        assertEquals(OutgoingLimits.Verdict.Ok, OutgoingLimits.check(draft()))
    }

    /** An ordinary photo passes, which is nearly every message. */
    @Test
    fun `an ordinary attachment passes`() {
        assertEquals(OutgoingLimits.Verdict.Ok, OutgoingLimits.check(draft(3_000_000)))
    }

    /**
     * **The limit is on the encoded message, not on the files.** base64
     * makes it a third larger, so 20 MB of photos is 27 MB on the wire
     * — and a person refused at that point reads it as the client
     * losing their message.
     */
    @Test
    fun `the encoding counts against the limit`() {
        // Under the raw limit, over the encoded one.
        val verdict = OutgoingLimits.check(draft(20_000_000))
        assertTrue(verdict.toString(), verdict is OutgoingLimits.Verdict.TooLarge)
    }

    /** Several files add up, because the server adds them up. */
    @Test
    fun `attachments are counted together`() {
        assertTrue(
            OutgoingLimits.check(draft(10_000_000, 10_000_000)) is OutgoingLimits.Verdict.TooLarge,
        )
        assertEquals(
            OutgoingLimits.Verdict.Ok,
            OutgoingLimits.check(draft(2_000_000, 2_000_000)),
        )
    }

    /**
     * Reported in the units the person chose the files in — they
     * attached 26 MB of photos, and telling them the message is 35 MB
     * is telling them about arithmetic they did not do.
     */
    @Test
    fun `the message names what was attached`() {
        val verdict = OutgoingLimits.check(draft(26_000_000)) as OutgoingLimits.Verdict.TooLarge
        assertEquals(26_000_000L, verdict.attachedBytes)
        assertTrue(verdict.limitBytes < OutgoingLimits.ENCODED_MAX)
    }

    /**
     * A server that states its own `SIZE` has told the truth where the
     * default has only guessed.
     */
    @Test
    fun `a server limit can replace the guess`() {
        assertTrue(
            OutgoingLimits.check(draft(3_000_000), limit = 1_000_000)
                is OutgoingLimits.Verdict.TooLarge,
        )
    }
}
