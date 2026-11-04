package jp.golia.mailrs.wire

import kotlinx.serialization.Serializable
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeParseException
import java.time.temporal.TemporalAdjusters

/**
 * The reader's calendar, a week at a time, read only.
 *
 * The rules are the web's `lib/calendar-grid.ts` and the iPhone's
 * `Wire/CalendarWeek.swift`: weeks start on Monday; a timed event with no end
 * lasts an hour, as the server reads it; an all-day event comes as dates
 * (`YYYY-MM-DD`, end exclusive) and covers those days from local midnight to
 * local midnight wherever the reader is — never placed through UTC.
 * Backend: `crates/webapi/src/handlers/calendar.rs` — `get_events` (`EventRow`).
 */
object CalendarWeek {
    @Serializable
    data class Event(
        val uid: String,
        val summary: String? = null,
        val dtstart: String? = null,
        val dtend: String? = null,
    )

    data class Span(val allDay: Boolean, val start: Instant, val end: Instant)

    /** The Monday on or before `day`. */
    fun startOfWeek(day: LocalDate): LocalDate = day.with(TemporalAdjusters.previousOrSame(DayOfWeek.MONDAY))

    fun days(monday: LocalDate): List<LocalDate> = (0L until 7L).map { monday.plusDays(it) }

    /** When the event is, in `zone`; null when it has no start. */
    fun span(ev: Event, zone: ZoneId): Span? {
        val dtstart = ev.dtstart ?: return null
        val first = localDate(dtstart)
        if (first != null) {
            val afterLast = localDate(ev.dtend.orEmpty()) ?: first.plusDays(1)
            return Span(true, first.atStartOfDay(zone).toInstant(), afterLast.atStartOfDay(zone).toInstant())
        }
        val start = instant(dtstart) ?: return null
        val end = ev.dtend?.let { instant(it) } ?: start.plusSeconds(3600)
        return Span(false, start, end)
    }

    /** The events touching `day`: all-day ones first, then by start. */
    fun eventsOn(events: List<Event>, day: LocalDate, zone: ZoneId): List<Event> {
        // a day is not always 24 hours where the clocks change
        val from = day.atStartOfDay(zone).toInstant()
        val to = day.plusDays(1).atStartOfDay(zone).toInstant()
        return events
            .mapNotNull { ev -> span(ev, zone)?.let { ev to it } }
            .filter { (_, s) -> s.start < to && s.end > from }
            .sortedWith(compareBy<Pair<Event, Span>>({ !it.second.allDay }, { it.second.start }))
            .map { it.first }
    }

    /** The window the server takes: the Monday to the Monday after, as instants. */
    fun window(monday: LocalDate, zone: ZoneId): Pair<String, String> =
        monday.atStartOfDay(zone).toInstant().toString() to monday.plusDays(7).atStartOfDay(zone).toInstant().toString()

    /** `2026-11-04`; null for anything else. */
    fun localDate(text: String): LocalDate? {
        if (text.length != 10) return null
        return try {
            LocalDate.parse(text)
        } catch (_: DateTimeParseException) {
            null
        }
    }

    private fun instant(text: String): Instant? = try {
        Instant.parse(text)
    } catch (_: DateTimeParseException) {
        null
    }
}

suspend fun MailrsClient.calendarEvents(monday: LocalDate, zone: ZoneId): MailrsClient.Outcome<List<CalendarWeek.Event>> {
    val (start, end) = CalendarWeek.window(monday, zone)
    return decode(get("/api/calendar/events?start=${enc(start)}&end=${enc(end)}"), CalendarWeek.Event.serializer())
}
