package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SendJoinTest {

    private fun sent(id: String, at: Long, subject: String = "Quarterly report") = Wire.SentMessage(
        uid = 1,
        messageId = id,
        threadId = "t1",
        to = "alice@example.com",
        subject = subject,
        internalDate = at,
    )

    private fun send(
        id: String,
        at: Long,
        status: String,
        resentFrom: String? = null,
    ) = Wire.Send(
        sendId = id,
        threadId = "t1",
        subject = "Quarterly report",
        to = listOf("alice@example.com"),
        createdAt = at,
        status = status,
        resentFrom = resentFrom,
    )

    @Test
    fun `a filed message takes the status of its send`() {
        val rows = SendJoin.join(
            messages = listOf(sent("<m1@x>", at = 100)),
            sends = listOf(send("m1@x", at = 100, status = "delivered")),
        )
        assertEquals(1, rows.size)
        // Brackets on one side only: the join must still find it, and
        // when it does not the row loses its status without a word.
        assertEquals("delivered", rows.single().status)
    }

    @Test
    fun `mail that predates the projection says nothing rather than delivered`() {
        val rows = SendJoin.join(listOf(sent("<old@x>", at = 50)), sends = emptyList())
        assertNull(rows.single().status)
    }

    @Test
    fun `a send the sweep has not filed yet still appears`() {
        val rows = SendJoin.join(
            messages = emptyList(),
            sends = listOf(send("fresh@x", at = 900, status = "queued")),
        )
        assertEquals("queued", rows.single().status)
        assertNull("nothing has a maildir uid yet", rows.single().uid)
    }

    @Test
    fun `a resend keeps the newest attempt against the original`() {
        val rows = SendJoin.join(
            messages = listOf(sent("<m1@x>", at = 100)),
            sends = listOf(
                send("m1@x", at = 100, status = "failed"),
                send("retry@x", at = 200, status = "delivered", resentFrom = "m1@x"),
            ),
        )
        assertEquals(1, rows.size)
        assertEquals("delivered", rows.single().status)
    }

    @Test
    fun `newest first`() {
        val rows = SendJoin.join(
            messages = listOf(sent("<a@x>", at = 100, subject = "older"), sent("<b@x>", at = 300, subject = "newer")),
            sends = emptyList(),
        )
        assertEquals(listOf("newer", "older"), rows.map { it.subject })
    }
}
