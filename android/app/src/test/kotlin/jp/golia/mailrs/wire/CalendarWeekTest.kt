package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate
import java.time.ZoneId

/** The same cases as the iPhone's `CalendarWeekTests`. */
class CalendarWeekTest {
    private val zones = listOf("Asia/Tokyo", "America/Los_Angeles", "Pacific/Kiritimati", "Pacific/Pago_Pago").map(ZoneId::of)
    private val offsite = CalendarWeek.Event("ev-offsite", "Team offsite", "2026-11-04", "2026-11-06")

    @Test
    fun weeks_start_on_monday() {
        assertEquals(LocalDate.of(2026, 11, 2), CalendarWeek.startOfWeek(LocalDate.of(2026, 11, 8)))
        assertEquals(LocalDate.of(2026, 11, 2), CalendarWeek.startOfWeek(LocalDate.of(2026, 11, 2)))
    }

    /** Dates, not instants: the same two days in every zone, the end day excluded. */
    @Test
    fun an_all_day_event_covers_its_dates_wherever_the_reader_is() {
        for (zone in zones) {
            val days = CalendarWeek.days(LocalDate.of(2026, 11, 2))
                .filter { CalendarWeek.eventsOn(listOf(offsite), it, zone).isNotEmpty() }
            assertEquals(zone.id, listOf(LocalDate.of(2026, 11, 4), LocalDate.of(2026, 11, 5)), days)
        }
    }

    @Test
    fun an_all_day_event_without_an_end_is_one_day() {
        val one = CalendarWeek.Event("a", null, "2026-11-04", null)
        val span = CalendarWeek.span(one, ZoneId.of("Asia/Tokyo"))!!
        assertTrue(span.allDay)
        assertEquals(86_400L, span.end.epochSecond - span.start.epochSecond)
    }

    @Test
    fun a_timed_event_without_an_end_lasts_an_hour() {
        val call = CalendarWeek.Event("b", null, "2026-11-04T00:00:00Z", null)
        val span = CalendarWeek.span(call, ZoneId.of("UTC"))!!
        assertEquals(3600L, span.end.epochSecond - span.start.epochSecond)
        assertEquals(false, span.allDay)
    }

    /** 00:00 UTC on Wednesday is Tuesday afternoon in Los Angeles. */
    @Test
    fun a_timed_event_is_placed_in_the_reader_zone() {
        val call = CalendarWeek.Event("b", null, "2026-11-04T00:00:00Z", "2026-11-04T00:30:00Z")
        assertEquals(1, CalendarWeek.eventsOn(listOf(call), LocalDate.of(2026, 11, 4), ZoneId.of("Asia/Tokyo")).size)
        assertEquals(1, CalendarWeek.eventsOn(listOf(call), LocalDate.of(2026, 11, 3), ZoneId.of("America/Los_Angeles")).size)
    }

    @Test
    fun all_day_events_come_first() {
        val timed = CalendarWeek.Event("t", null, "2026-11-04T00:00:00Z", null)
        val order = CalendarWeek.eventsOn(listOf(timed, offsite), LocalDate.of(2026, 11, 4), ZoneId.of("Asia/Tokyo")).map { it.uid }
        assertEquals(listOf("ev-offsite", "t"), order)
    }

    @Test
    fun an_event_with_no_start_is_not_placed() {
        assertNull(CalendarWeek.span(CalendarWeek.Event("x"), ZoneId.of("UTC")))
        assertNull(CalendarWeek.span(CalendarWeek.Event("y", null, "next week", null), ZoneId.of("UTC")))
    }

    @Test
    fun the_window_is_the_local_week_as_instants() {
        val (start, end) = CalendarWeek.window(LocalDate.of(2026, 11, 2), ZoneId.of("Asia/Tokyo"))
        assertEquals("2026-11-01T15:00:00Z", start)
        assertEquals("2026-11-08T15:00:00Z", end)
    }
}
