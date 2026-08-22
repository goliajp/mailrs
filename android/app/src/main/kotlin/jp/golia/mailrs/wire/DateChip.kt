package jp.golia.mailrs.wire

import java.time.LocalDate
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/**
 * One shape for every date chip, whatever the writer typed.
 *
 * A suggestion quotes the source text back, which is honest and is what
 * an inline underline would show. Out of context in a row it reads as
 * noise: `Aug 21 2026` beside `2026-08-20` beside `2026-08-21` looks
 * like three unrelated things rather than three days. The written form
 * stays as the row's content description and as the event's title, so
 * nothing is lost — only the row is made readable.
 *
 * Parsed as a local date, never as an instant: a bare `YYYY-MM-DD` read
 * as UTC midnight renders as the day before for any reader west of
 * Greenwich.
 */
object DateChip {
    fun label(s: Wire.DateSuggestion, locale: Locale = Locale.getDefault()): String {
        val day = runCatching { LocalDate.parse(s.date) }.getOrNull() ?: return s.text
        val at = s.datetime?.let { runCatching { LocalDateTime.parse(it) }.getOrNull() }
        val style =
            if (at == null) {
                DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)
            } else {
                DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
            }
        return style.withLocale(locale).format(at ?: day)
    }
}
