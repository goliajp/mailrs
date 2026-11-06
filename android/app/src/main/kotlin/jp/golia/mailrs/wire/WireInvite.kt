package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/**
 * A meeting invitation, as the server read it out of the message.
 *
 * Backend: `crates/webapi/src/handlers/complete.rs` —
 * `get_message_single`'s `invite_payload`, with what
 * `calendar_events::series::annotate` adds when it is read.
 */
@Serializable
data class WireInvite(
    val uid: String = "",
    /**
     * Higher on every re-send. Exchange does not send
     * `METHOD:UPDATE` — it re-sends the whole invitation as a
     * `REQUEST` with a higher sequence — so this is what tells an
     * update from a first invitation.
     */
    val sequence: Int = 0,
    val summary: String = "",
    val location: String? = null,
    val organizer: WirePerson? = null,
    val attendees: List<WireInviteAttendee> = emptyList(),
    val status: String? = null,
    /**
     * **The instant, resolved on the server** against the
     * invitation's own `VTIMEZONE`, RFC 3339.
     *
     * Read this, not the wall-clock. A `TZID` is routinely a
     * Windows name — `Pacific Standard Time`, which says "Standard"
     * while the event is in daylight time — and no client-side
     * parser can evaluate one. Null for an all-day event, which has
     * no instant: a date has no offset, and giving it one moves it
     * a day.
     */
    @SerialName("dtstart_utc") val startsAt: String? = null,
    @SerialName("dtend_utc") val endsAt: String? = null,
    /**
     * An all-day event's first day, `YYYY-MM-DD`, and the day after
     * its last (DTEND, as RFC 5545 writes it). Null for a timed event,
     * which has [startsAt] instead.
     */
    @SerialName("dtstart_date") val firstDay: String? = null,
    @SerialName("dtend_date") val dayAfterLast: String? = null,
    /**
     * The way into the meeting, resolved on the server: RFC 5545
     * has no field for it, so Teams writes it into the description
     * and Zoom into the location, and one implementation of "which
     * URL is a meeting" beats three.
     */
    @SerialName("join_url") val joinUrl: String? = null,
    /** The wall-clock and zone the organiser wrote, for the second line. */
    val dtstart: JsonElement? = null,
    /**
     * The IANA zone the organiser's `TZID` names, Windows names resolved
     * on the server. The organiser's clock is formatted from the instant
     * in it, so the abbreviation is the one in force that day.
     */
    @SerialName("organiser_zone") val organiserZone: String? = null,
    /** How a recurring invitation repeats; null for one that does not. */
    val recurrence: WireRecurrence? = null,
    /**
     * The first occurrence that has not ended, worked out by the server
     * when the message is read. Null for a single event, and for a series
     * that is over (then [recurrence] is still set).
     */
    @SerialName("next_occurrence") val next: WireOccurrence? = null,
)

/** A rule, read for showing in words (`mailrs_ical::recur::Rule`). */
@Serializable
data class WireRecurrence(
    val freq: String = "",
    val interval: Int = 1,
    @SerialName("by_day") val byDay: List<String> = emptyList(),
    @SerialName("by_month_day") val byMonthDay: List<Int> = emptyList(),
    val count: Int? = null,
    /** The last day, `YYYY-MM-DD`, in the organiser's zone. */
    val until: String? = null,
    val plain: Boolean = false,
)

/** One occurrence: RFC 3339 instants, or dates for an all-day series. */
@Serializable
data class WireOccurrence(
    val start: String = "",
    val end: String = "",
    @SerialName("organiser_local") val organiserLocal: String? = null,
)

@Serializable
data class WirePerson(val cn: String? = null, val email: String = "")

@Serializable
data class WireInviteAttendee(
    val cn: String? = null,
    val email: String = "",
    /** `NEEDS-ACTION` / `ACCEPTED` / `DECLINED` / `TENTATIVE`. */
    val partstat: String = "NEEDS-ACTION",
)
