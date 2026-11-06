package jp.golia.mailrs.wire

import jp.golia.mailrs.R
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.format.TextStyle
import java.util.Locale

/**
 * When an invitation is, as the card says it: the reader's own time, the
 * organiser's beside it, and for a recurring meeting how it repeats.
 *
 * The rule is expanded on the server, which sends the next occurrence as
 * an instant; this only formats. Every instant goes through `java.time`'s
 * zone database at that instant — the reader's zone for the main line,
 * the organiser's for the second — so neither side's clock change is
 * applied from a fixed offset. The same rules as the web's
 * `lib/invite-recurrence.ts` and the iPhone's `Wire/InviteRepeat.swift`.
 */
object InviteRepeat {
    private val DAYS = mapOf(
        "MO" to DayOfWeek.MONDAY,
        "TU" to DayOfWeek.TUESDAY,
        "WE" to DayOfWeek.WEDNESDAY,
        "TH" to DayOfWeek.THURSDAY,
        "FR" to DayOfWeek.FRIDAY,
        "SA" to DayOfWeek.SATURDAY,
        "SU" to DayOfWeek.SUNDAY,
    )

    /** The time line: the next occurrence of a series, else the event itself. */
    fun whenLine(invite: WireInvite, reader: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String? {
        val next = invite.next
        val start = next?.start ?: invite.startsAt
        val local = InviteRules.localTime(start, reader)
            ?: return allDay(invite)
        val line = when (next) {
            null -> local
            else -> Texts.get(R.string.invite_next, local)
        }
        val organiser = organiserClock(start, invite, reader, locale) ?: return line
        return "$line · $organiser"
    }

    /** "每周四 · 到 2027年4月29日 为止"; null for an invitation that does not repeat. */
    fun ruleLine(invite: WireInvite, reader: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String? {
        val rule = invite.recurrence ?: return null
        val parts = mutableListOf(words(rule, firstDayOf(invite), locale))
        rule.until?.let { day ->
            runCatching { LocalDate.parse(day) }.getOrNull()?.let {
                val date = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(it)
                parts.add(Texts.get(R.string.invite_repeat_until, date))
            }
        }
        rule.count?.let { parts.add(Texts.get(R.string.invite_repeat_count, it)) }
        var line = parts.joinToString(" · ")
        if (organiserZoneDiffers(invite, reader)) line += Texts.get(R.string.invite_organiser_time)
        if (invite.next == null) line += " · " + Texts.get(R.string.invite_repeat_ended)
        return line
    }

    /**
     * "22:00 PDT": the instant in the organiser's IANA zone, with the
     * abbreviation in force on that date. Null when the reader is in that
     * zone, or when the server could not name one and there is no
     * wall-clock to fall back on.
     */
    fun organiserClock(start: String?, invite: WireInvite, reader: ZoneId, locale: Locale): String? {
        val zone = invite.organiserZone?.let { runCatching { ZoneId.of(it) }.getOrNull() }
        val at = start?.let { runCatching { Instant.parse(it) }.getOrNull() }
        if (zone != null && at != null) {
            if (zone.normalized() == reader.normalized() || zone.id == reader.id) return null
            val time = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).withZone(zone).format(at)
            val abbreviation = DateTimeFormatter.ofPattern("zzz", Locale.US).withZone(zone).format(at)
            return "$time $abbreviation"
        }
        // no IANA name: the wall-clock the server read off the invitation, with its TZID
        val tzName = zoneNameOf(invite.dtstart) ?: return null
        val wall = invite.next?.organiserLocal ?: wallClockOf(invite.dtstart) ?: return null
        if (!InviteRules.zoneDiffers(tzName, reader)) return null
        return "${wall.drop(11).take(5)} $tzName"
    }

    private fun organiserZoneDiffers(invite: WireInvite, reader: ZoneId): Boolean {
        val zone = invite.organiserZone ?: return InviteRules.zoneDiffers(zoneNameOf(invite.dtstart), reader)
        return zone != reader.id
    }

    private fun allDay(invite: WireInvite): String? {
        val next = invite.next ?: return InviteRules.allDayLine(invite.firstDay, invite.dayAfterLast)
        val days = InviteRules.allDayLine(next.start, next.end) ?: return null
        return Texts.get(R.string.invite_next, days)
    }

    private fun words(rule: WireRecurrence, firstDay: LocalDate?, locale: Locale): String {
        if (!rule.plain || rule.interval < 1) return Texts.get(R.string.invite_repeat_other)
        val n = rule.interval
        return when (rule.freq) {
            "DAILY" -> when (n) {
                1 -> Texts.get(R.string.invite_repeat_daily)
                else -> Texts.get(R.string.invite_repeat_daily_n, n)
            }
            "WEEKLY" -> weekly(rule, firstDay, locale)
            "MONTHLY" -> when {
                n == 1 && rule.byMonthDay.size == 1 && rule.byDay.isEmpty() ->
                    Texts.get(R.string.invite_repeat_monthly_day, rule.byMonthDay[0])
                n == 1 -> Texts.get(R.string.invite_repeat_monthly)
                else -> Texts.get(R.string.invite_repeat_other)
            }
            "YEARLY" -> when (n) {
                1 -> Texts.get(R.string.invite_repeat_yearly)
                else -> Texts.get(R.string.invite_repeat_other)
            }
            else -> Texts.get(R.string.invite_repeat_other)
        }
    }

    private fun weekly(rule: WireRecurrence, firstDay: LocalDate?, locale: Locale): String {
        val codes = when (rule.byDay.isEmpty()) {
            true -> listOfNotNull(firstDay?.dayOfWeek)
            false -> rule.byDay.map { DAYS[it] }
        }
        if (codes.isEmpty() || codes.any { it == null }) return Texts.get(R.string.invite_repeat_other)
        // Chinese says 周四 inside 每周四; the other languages use the full name
        val style = when (locale.language) {
            "zh" -> TextStyle.SHORT
            else -> TextStyle.FULL
        }
        val names = codes.filterNotNull().joinToString(Texts.get(R.string.invite_repeat_day_separator)) {
            it.getDisplayName(style, locale)
        }
        return when (rule.interval) {
            1 -> Texts.get(R.string.invite_repeat_weekly, names)
            else -> Texts.get(R.string.invite_repeat_weekly_n, rule.interval, names)
        }
    }

    /** The organiser's first day, for a weekly rule that names no weekday. */
    private fun firstDayOf(invite: WireInvite): LocalDate? {
        val wall = wallClockOf(invite.dtstart) ?: return null
        return runCatching { LocalDate.parse(wall.take(10)) }.getOrNull()
    }

    internal fun zoneNameOf(dtstart: kotlinx.serialization.json.JsonElement?): String? = zonedField(dtstart, "tz_name")

    internal fun wallClockOf(dtstart: kotlinx.serialization.json.JsonElement?): String? = zonedField(dtstart, "local")

    private fun zonedField(dtstart: kotlinx.serialization.json.JsonElement?, name: String): String? {
        val zoned = (dtstart as? kotlinx.serialization.json.JsonObject)?.get("Zoned")
        val field = (zoned as? kotlinx.serialization.json.JsonObject)?.get(name)
        return (field as? kotlinx.serialization.json.JsonPrimitive)?.content
    }
}
