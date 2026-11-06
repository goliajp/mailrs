package jp.golia.mailrs.wire

import androidx.annotation.StringRes
import jp.golia.mailrs.R
import java.time.DayOfWeek
import java.time.ZonedDateTime

/**
 * Putting a conversation away until later: the iPhone's four choices
 * (`ios/Mailrs/Wire/SnoozeChoice.swift`), with the same times and words.
 *
 * Read in the phone's own zone: "morning" is 8:00 where the reader is,
 * because a thread that comes back in the middle of the night is worse
 * than one that never left.
 */
enum class SnoozeChoice(@StringRes private val labelId: Int) {
    LaterToday(R.string.snooze_later_today),
    TomorrowMorning(R.string.snooze_tomorrow_morning),
    ThisWeekend(R.string.snooze_this_weekend),
    NextWeek(R.string.snooze_next_week),
    ;

    val label: String get() = Texts.get(labelId)

    /** Epoch seconds. */
    fun fireDate(now: ZonedDateTime): Long = when (this) {
        // Three hours on, not an evening clock time: chosen at 11pm there
        // is no evening left, and rolling to tomorrow is a different promise.
        LaterToday -> now.plusHours(3)
        TomorrowMorning -> morning(now.plusDays(1))
        ThisWeekend -> morning(next(DayOfWeek.SATURDAY, now))
        NextWeek -> morning(next(DayOfWeek.MONDAY, now))
    }.toEpochSecond()

    private fun morning(day: ZonedDateTime): ZonedDateTime =
        day.withHour(8).withMinute(0).withSecond(0).withNano(0)

    /** The next such weekday strictly after [now], so never today. */
    private fun next(weekday: DayOfWeek, now: ZonedDateTime): ZonedDateTime {
        var day = now.plusDays(1)
        while (day.dayOfWeek != weekday) day = day.plusDays(1)
        return day
    }
}
