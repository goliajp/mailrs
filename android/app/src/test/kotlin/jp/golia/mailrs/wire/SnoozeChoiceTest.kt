package jp.golia.mailrs.wire

import java.time.DayOfWeek
import java.time.Instant
import java.time.ZoneId
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/** The same choices and times as `ios/Mailrs/Wire/SnoozeChoice.swift` and its tests. */
class SnoozeChoiceTest {
    private val tokyo = ZoneId.of("Asia/Tokyo")

    @Before
    fun englishWords() = TestTexts.install()

    private fun at(epoch: Long): ZonedDateTime = ZonedDateTime.ofInstant(Instant.ofEpochSecond(epoch), tokyo)

    @Test
    fun the_four_choices_are_the_iphones_in_its_order_and_words() {
        assertEquals(
            listOf("Later today", "Tomorrow morning", "This weekend", "Next week"),
            SnoozeChoice.entries.map { it.label },
        )
    }

    @Test
    fun every_morning_choice_is_eight_oclock_local_and_in_the_future() {
        // Friday 2026-11-06 23:30 in Tokyo
        val now = ZonedDateTime.of(2026, 11, 6, 23, 30, 0, 0, tokyo)
        for (choice in listOf(SnoozeChoice.TomorrowMorning, SnoozeChoice.ThisWeekend, SnoozeChoice.NextWeek)) {
            val fire = at(choice.fireDate(now))
            assertEquals("${choice.label} is not a morning", 8, fire.hour)
            assertEquals(0, fire.minute)
            assertTrue("${choice.label} is in the past", fire.isAfter(now))
        }
    }

    @Test
    fun the_weekend_is_saturday_and_next_week_is_monday() {
        val wednesday = ZonedDateTime.of(2026, 11, 4, 10, 0, 0, 0, tokyo)
        assertEquals(DayOfWeek.SATURDAY, at(SnoozeChoice.ThisWeekend.fireDate(wednesday)).dayOfWeek)
        assertEquals(7, at(SnoozeChoice.ThisWeekend.fireDate(wednesday)).dayOfMonth)
        assertEquals(DayOfWeek.MONDAY, at(SnoozeChoice.NextWeek.fireDate(wednesday)).dayOfWeek)
        assertEquals(9, at(SnoozeChoice.NextWeek.fireDate(wednesday)).dayOfMonth)
        assertEquals(5, at(SnoozeChoice.TomorrowMorning.fireDate(wednesday)).dayOfMonth)
    }

    /** Said on a Monday, "next week" is the Monday coming, not the one that began this morning. */
    @Test
    fun a_weekday_choice_is_never_today() {
        val monday = ZonedDateTime.of(2026, 11, 9, 0, 1, 0, 0, tokyo)
        val fire = SnoozeChoice.NextWeek.fireDate(monday)
        assertTrue(fire - monday.toEpochSecond() > 6 * 24 * 60 * 60)
    }

    @Test
    fun later_today_is_three_hours_on_even_at_night() {
        val night = ZonedDateTime.of(2026, 11, 6, 23, 0, 0, 0, tokyo)
        assertEquals(night.toEpochSecond() + 3 * 60 * 60, SnoozeChoice.LaterToday.fireDate(night))
    }
}
