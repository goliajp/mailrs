package jp.golia.mailrs.wire

import java.time.DayOfWeek
import java.time.ZonedDateTime

/**
 * When a message should leave.
 *
 * Ported from `ios/Mailrs/Wire/SendSchedule.swift`. The server has
 * taken `scheduled_at` since scheduling existed and the sender sweeps a
 * score-ordered set and promotes what is due; the phone is the device
 * most likely to be writing mail at an hour nobody wants it delivered,
 * and it had no way to ask.
 */
enum class SendSchedule(val label: String) {
    Now("Send now"),
    LaterToday("Later today"),
    TomorrowMorning("Tomorrow morning"),
    MondayMorning("Monday morning"),
    ;

    /**
     * Epoch **seconds**, or null for now.
     *
     * Seconds and an integer, both deliberate: the handler reads
     * anything it cannot parse as "not scheduling", which is how the
     * web's ISO 8601 string once made every scheduled send go out at
     * once. It is a 400 today, and this side never has the chance to
     * produce one.
     */
    fun fireDate(now: ZonedDateTime): Long? = when (this) {
        Now -> null
        // Three hours on, not a clock time: "later today" chosen at
        // 11pm has no evening left to land in, and rolling it to
        // tomorrow would be a different choice than the one made.
        LaterToday -> now.plusHours(3).toEpochSecond()
        TomorrowMorning -> morning(now.plusDays(1)).toEpochSecond()
        MondayMorning -> morning(nextMonday(now)).toEpochSecond()
    }

    /**
     * 8am where the phone is, not 8am UTC — a schedule named after a
     * time of day and delivered in the middle of the night is worse
     * than no scheduling at all.
     */
    private fun morning(day: ZonedDateTime): ZonedDateTime =
        day.withHour(8).withMinute(0).withSecond(0).withNano(0)

    /**
     * The next Monday strictly after [now] — never today, even at one
     * minute past midnight on a Monday, because "Monday morning" said
     * on a Monday means the one coming.
     */
    private fun nextMonday(now: ZonedDateTime): ZonedDateTime {
        var day = now.plusDays(1)
        while (day.dayOfWeek != DayOfWeek.MONDAY) day = day.plusDays(1)
        return day
    }
}
