package jp.golia.mailrs.wire

import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InviteIntentTest {
    private val tokyo = ZoneId.of("Asia/Tokyo")

    /**
     * A time written in prose is local to whoever reads it — "2pm" in a
     * sentence means the writer's own afternoon, and neither side knows
     * which zone that was. Resolving it against the reader's zone when
     * it is filed is the honest reading; stamping it UTC would move the
     * appointment by the reader's own offset.
     */
    @Test
    fun a_written_time_is_read_as_the_readers_own() {
        val start = InviteIntent.whenOf(
            Wire.DateSuggestion(date = "2026-08-21", datetime = "2026-08-21T14:00:00", text = "2pm"),
            tokyo,
        )
        assertFalse(start.allDay)
        // 14:00 in Tokyo is 05:00 UTC.
        assertEquals(1_787_288_400_000L, start.startMillis)
    }

    /** A day with no hour is a day. Midnight is a meeting time nobody wrote. */
    @Test
    fun a_day_with_no_hour_stays_a_day() {
        val start = InviteIntent.whenOf(
            Wire.DateSuggestion(date = "2026-08-21", datetime = null, text = "21 August"),
            tokyo,
        )
        assertTrue(start.allDay)
        assertEquals(1_787_238_000_000L, start.startMillis)
    }

    /** Unreadable input does not crash the card it is drawn on. */
    @Test
    fun nonsense_falls_back_rather_than_throwing() {
        val start = InviteIntent.whenOf(
            Wire.DateSuggestion(date = "not-a-date", datetime = "neither", text = "?"),
            tokyo,
        )
        assertTrue(start.allDay)
        assertTrue(start.startMillis > 0)
    }
}
