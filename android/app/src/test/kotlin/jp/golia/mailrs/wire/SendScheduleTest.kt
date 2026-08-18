package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.ZoneId
import java.time.ZonedDateTime

class SendScheduleTest {

    private val tokyo = ZoneId.of("Asia/Tokyo")

    private fun at(text: String) = ZonedDateTime.parse(text).withZoneSameInstant(tokyo)

    @Test
    fun `now schedules nothing`() {
        assertNull(SendSchedule.Now.fireDate(at("2026-08-18T23:10:00+09:00")))
    }

    @Test
    fun `later today is three hours on, even at eleven at night`() {
        val now = at("2026-08-18T23:10:00+09:00")
        val fire = SendSchedule.LaterToday.fireDate(now)!!
        assertEquals(now.plusHours(3).toEpochSecond(), fire)
        // Deliberately allowed to land tomorrow: rolling it back to
        // this evening would be a time that has already gone, which
        // the handler refuses with a 400.
        assertTrue(fire > now.toEpochSecond())
    }

    @Test
    fun `tomorrow morning is eight where the phone is`() {
        val fire = SendSchedule.TomorrowMorning.fireDate(at("2026-08-18T23:10:00+09:00"))!!
        val landed = ZonedDateTime.ofInstant(java.time.Instant.ofEpochSecond(fire), tokyo)
        assertEquals(8, landed.hour)
        assertEquals(19, landed.dayOfMonth)
    }

    @Test
    fun `monday morning said on a monday means the one coming`() {
        // 2026-08-17 is a Monday.
        val fire = SendSchedule.MondayMorning.fireDate(at("2026-08-17T00:01:00+09:00"))!!
        val landed = ZonedDateTime.ofInstant(java.time.Instant.ofEpochSecond(fire), tokyo)
        assertEquals(java.time.DayOfWeek.MONDAY, landed.dayOfWeek)
        assertEquals(24, landed.dayOfMonth)
        assertEquals(8, landed.hour)
    }

    @Test
    fun `every choice is in the future`() {
        val now = at("2026-08-18T23:59:00+09:00")
        for (choice in SendSchedule.entries) {
            val fire = choice.fireDate(now) ?: continue
            assertTrue("${choice.label} is in the past", fire > now.toEpochSecond())
        }
    }
}
