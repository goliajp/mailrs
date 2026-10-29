package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Test

/** How long a read waits before asking a restarting server again. */
class RetryAfterTest {
    @Test
    fun `the server's seconds are used, capped at thirty`() {
        assertEquals(1_000L, retryAfterMs("1"))
        assertEquals(5_000L, retryAfterMs(" 5 "))
        assertEquals(30_000L, retryAfterMs("3600"))
    }

    @Test
    fun `no usable value waits five seconds`() {
        assertEquals(5_000L, retryAfterMs(null))
        assertEquals(5_000L, retryAfterMs("0"))
        assertEquals(5_000L, retryAfterMs("-3"))
        // the HTTP-date form is not what the server sends; it is not guessed at
        assertEquals(5_000L, retryAfterMs("Wed, 21 Oct 2026 07:28:00 GMT"))
    }
}
