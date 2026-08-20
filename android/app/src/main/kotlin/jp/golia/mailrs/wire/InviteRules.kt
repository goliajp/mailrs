package jp.golia.mailrs.wire

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

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
        "CANCEL" -> "Cancelled"
        "COUNTER" -> "Counter-proposed"
        "PUBLISH" -> "Event"
        "REPLY" -> "Reply"
        "REQUEST" -> if (sequence > 0) "Updated invite" else "New invite"
        "UPDATE" -> "Updated invite"
        else -> "Invitation"
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
    fun guests(attendees: List<Wire.InviteAttendee>): String {
        val head = if (attendees.size == 1) "1 guest" else "${attendees.size} guests"
        val yes = attendees.count { it.partstat.uppercase() == "ACCEPTED" }
        val no = attendees.count { it.partstat.uppercase() == "DECLINED" }
        val waiting = attendees.count { it.partstat.uppercase() == "NEEDS-ACTION" }
        val parts = buildList {
            if (yes > 0) add("$yes yes")
            if (no > 0) add("$no no")
            if (waiting > 0) add("$waiting awaiting")
        }
        return if (parts.isEmpty()) head else "$head · ${parts.joinToString(", ")}"
    }

    /** What this reader already said. */
    fun answered(partstat: String): String = when (partstat.uppercase()) {
        "ACCEPTED" -> "You accepted"
        "DECLINED" -> "You declined"
        "TENTATIVE" -> "You answered maybe"
        else -> "You answered"
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
