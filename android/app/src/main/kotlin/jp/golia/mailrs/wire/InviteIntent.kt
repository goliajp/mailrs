package jp.golia.mailrs.wire

import android.content.Intent
import android.provider.CalendarContract
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId

/**
 * Handing a proposed event to the calendar.
 *
 * `ACTION_INSERT` opens the calendar's own new-event screen with the
 * fields filled in: the reader sees what is about to be saved, and no
 * calendar permission is needed because nothing is written on their
 * behalf. Writing it directly would need `WRITE_CALENDAR` and would
 * file a guess.
 */
object InviteIntent {
    /**
     * The intent for one suggestion.
     *
     * A time written in prose is **local to whoever reads it** — "2pm"
     * in a sentence means the writer's own afternoon and neither side
     * knows which zone that was, which is the same thing RFC 5545 calls
     * a floating time. So it is resolved against the reader's zone
     * here, at the moment it is filed, rather than stamped with an
     * offset nobody stated.
     *
     * A day with no hour stays a day: `ALL_DAY`, because giving it
     * midnight invents a meeting time nobody wrote.
     */
    fun insert(s: Wire.DateSuggestion, zone: ZoneId = ZoneId.systemDefault()): Intent {
        val when_ = whenOf(s, zone)
        val intent = Intent(Intent.ACTION_INSERT)
            .setData(CalendarContract.Events.CONTENT_URI)
            .putExtra(CalendarContract.Events.TITLE, s.text)
            .putExtra(CalendarContract.EXTRA_EVENT_BEGIN_TIME, when_.startMillis)
        if (when_.allDay) {
            intent.putExtra(CalendarContract.EXTRA_EVENT_ALL_DAY, true)
        } else {
            intent.putExtra(CalendarContract.EXTRA_EVENT_END_TIME, when_.startMillis + 3_600_000)
        }
        return intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }

    /** When the event starts, and whether it has an hour at all. */
    data class Start(val startMillis: Long, val allDay: Boolean)

    /**
     * Split out from [insert] so it can be tested without an Android
     * runtime: the arithmetic is the part that can be wrong, and
     * `Intent` needs a device or Robolectric, which this project does
     * not carry for one test.
     */
    fun whenOf(s: Wire.DateSuggestion, zone: ZoneId): Start {
        val at = s.datetime?.let { runCatching { LocalDateTime.parse(it) }.getOrNull() }
        if (at != null) {
            return Start(at.atZone(zone).toInstant().toEpochMilli(), allDay = false)
        }
        val day = runCatching { LocalDate.parse(s.date) }.getOrNull() ?: LocalDate.now(zone)
        return Start(day.atStartOfDay(zone).toInstant().toEpochMilli(), allDay = true)
    }
}
