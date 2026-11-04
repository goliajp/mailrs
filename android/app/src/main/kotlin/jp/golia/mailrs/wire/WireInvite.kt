package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/**
 * A meeting invitation, as the server read it out of the message.
 *
 * Backend: `crates/webapi/src/handlers/complete.rs` —
 * `get_message_single`'s `invite_payload`.
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
