package jp.golia.mailrs.wire

import jp.golia.mailrs.R
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/**
 * What kind of invitation this is, and whether it wants an answer.
 *
 * The same rules the web and iOS apply, kept out of the composable so
 * they can be tested without a screen — and stated once per platform
 * rather than guessed at twice.
 */
object InviteRules {
    /**
     * The label above the card.
     *
     * `METHOD:UPDATE` exists in RFC 5546 and almost nobody sends it:
     * Exchange re-sends the whole invitation as a `REQUEST` with a
     * higher `SEQUENCE`, which is how a meeting that has moved three
     * times arrives. Calling that a new invitation tells the reader the
     * opposite of what happened.
     */
    fun badge(method: String, sequence: Int): String = when (method.uppercase()) {
        "CANCEL" -> Texts.get(R.string.invite_cancelled)
        "COUNTER" -> Texts.get(R.string.invite_counter)
        "PUBLISH" -> Texts.get(R.string.invite_event)
        "REPLY" -> Texts.get(R.string.invite_reply)
        "REQUEST" -> when (sequence > 0) {
            true -> Texts.get(R.string.invite_updated)
            false -> Texts.get(R.string.invite_new)
        }
        "UPDATE" -> Texts.get(R.string.invite_updated)
        else -> Texts.get(R.string.invite_invitation)
    }

    /**
     * Whether to offer Yes / Maybe / No.
     *
     * Only a `REQUEST` asks the reader anything. A `PUBLISH` is a notice
     * and a `REPLY` is somebody else's answer arriving; answering either
     * sends an iTIP message to a party that did not ask for one.
     */
    fun wantsAnswer(method: String): Boolean = method.uppercase() == "REQUEST"

    /** "4 guests · 1 yes, 2 awaiting" — the count alone does not say whether it is happening. */
    fun guests(attendees: List<WireInviteAttendee>): String {
        val head = when (attendees.size) {
            1 -> Texts.get(R.string.invite_one_guest)
            else -> Texts.get(R.string.invite_guests, attendees.size)
        }
        val yes = attendees.count { it.partstat.uppercase() == "ACCEPTED" }
        val no = attendees.count { it.partstat.uppercase() == "DECLINED" }
        val waiting = attendees.count { it.partstat.uppercase() == "NEEDS-ACTION" }
        val parts = buildList {
            if (yes > 0) add(Texts.get(R.string.invite_count_yes, yes))
            if (no > 0) add(Texts.get(R.string.invite_count_no, no))
            if (waiting > 0) add(Texts.get(R.string.invite_count_waiting, waiting))
        }
        return when (parts.isEmpty()) {
            true -> head
            false -> "$head · ${parts.joinToString(Texts.get(R.string.list_separator))}"
        }
    }

    /** What this reader already said. */
    fun answered(partstat: String): String = when (partstat.uppercase()) {
        "ACCEPTED" -> Texts.get(R.string.invite_you_accepted)
        "DECLINED" -> Texts.get(R.string.invite_you_declined)
        "TENTATIVE" -> Texts.get(R.string.invite_you_maybe)
        else -> Texts.get(R.string.invite_you_answered)
    }

    /**
     * The reader's own time. Null for an all-day event, which carries no
     * instant and must not be given one.
     */
    fun localTime(rfc3339: String?, zone: ZoneId = ZoneId.systemDefault()): String? {
        val at = runCatching { Instant.parse(rfc3339 ?: return null) }.getOrNull() ?: return null
        return DateTimeFormatter
            .ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
            .withZone(zone)
            .format(at)
    }

    /**
     * An all-day event's days, as written, in the reader's language and
     * without a time. A date has no offset, so it is never moved into a
     * zone — read as midnight somewhere, it would land on the day before
     * for everyone west of there. [dayAfterLast] is DTEND, the day after
     * the last: `2026-11-03` to `2026-11-05` is the 3rd and the 4th.
     */
    fun allDayLine(firstDay: String?, dayAfterLast: String?, locale: Locale = Locale.getDefault()): String? {
        val first = runCatching { LocalDate.parse(firstDay ?: return null) }.getOrNull() ?: return null
        val format = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale)
        val last = dayAfterLast
            ?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
            ?.minusDays(1)
        if (last == null || !last.isAfter(first)) return format.format(first)
        return format.format(first) + " – " + format.format(last)
    }

    /**
     * Whether the organiser's zone is worth naming beside the reader's.
     *
     * Exchange writes Windows zone names, which `ZoneId` does not know,
     * so the comparison is against the handful that turn up plus the
     * IANA name itself.
     */
    fun zoneDiffers(zone: String?, reader: ZoneId = ZoneId.systemDefault()): Boolean {
        if (zone.isNullOrBlank()) return false
        if (zone == reader.id) return false
        return WINDOWS_TO_IANA[zone] != reader.id
    }

    private val WINDOWS_TO_IANA = mapOf(
        "Central Standard Time" to "America/Chicago",
        "China Standard Time" to "Asia/Shanghai",
        "Eastern Standard Time" to "America/New_York",
        "GMT Standard Time" to "Europe/London",
        "Pacific Standard Time" to "America/Los_Angeles",
        "Tokyo Standard Time" to "Asia/Tokyo",
        "W. Europe Standard Time" to "Europe/Berlin",
    )
}
