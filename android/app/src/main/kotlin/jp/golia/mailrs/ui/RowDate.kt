package jp.golia.mailrs.ui

import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/**
 * The date a list row wears — Apple Mail's ladder, the same one
 * `ios/Mailrs/Wire/RowDate.swift` walks.
 *
 * The ladder front-loads whatever changes a decision: mail from today
 * shows the **time** (open now, or after the meeting?), this week the
 * weekday, and only past that a date. A flat "Aug 5" on today's mail
 * hides exactly the freshness that decides.
 *
 * Pure and injected, so the ladder is testable without freezing a
 * locale or a clock into the output.
 */
object RowDate {

    enum class Bucket { TIME, YESTERDAY, WEEKDAY, SAME_YEAR_DATE, DATED_WITH_YEAR }

    fun bucket(date: LocalDate, today: LocalDate): Bucket = when {
        date == today -> Bucket.TIME
        date == today.minusDays(1) -> Bucket.YESTERDAY
        !date.isBefore(today.minusDays(6)) && !date.isAfter(today) -> Bucket.WEEKDAY
        date.year == today.year -> Bucket.SAME_YEAR_DATE
        else -> Bucket.DATED_WITH_YEAR
    }

    fun format(
        epochSeconds: Long,
        zone: ZoneId = ZoneId.systemDefault(),
        today: LocalDate = LocalDate.now(zone),
        locale: Locale = Locale.getDefault(),
    ): String {
        if (epochSeconds <= 0) return ""
        val moment = Instant.ofEpochSecond(epochSeconds).atZone(zone)
        return when (bucket(moment.toLocalDate(), today)) {
            Bucket.TIME ->
                DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(moment)
            // "Yesterday" in the reader's language, which is what
            // `doesRelativeDateFormatting` gives on the other client.
            Bucket.YESTERDAY -> android.text.format.DateUtils.getRelativeTimeSpanString(
                epochSeconds * 1000,
                System.currentTimeMillis(),
                android.text.format.DateUtils.DAY_IN_MILLIS,
                android.text.format.DateUtils.FORMAT_ABBREV_RELATIVE,
            ).toString()
            Bucket.WEEKDAY ->
                DateTimeFormatter.ofPattern("EEE", locale).format(moment)
            Bucket.SAME_YEAR_DATE ->
                DateTimeFormatter.ofPattern("MMM d", locale).format(moment)
            Bucket.DATED_WITH_YEAR ->
                DateTimeFormatter.ofLocalizedDate(FormatStyle.SHORT).withLocale(locale).format(moment)
        }
    }
}
