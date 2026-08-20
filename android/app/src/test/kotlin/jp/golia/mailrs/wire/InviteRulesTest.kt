package jp.golia.mailrs.wire

import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class InviteRulesTest {
    /**
     * The case that matters, because it is the common one: Exchange
     * does not send METHOD:UPDATE. It re-sends the whole invitation as
     * a REQUEST with a higher SEQUENCE, so a meeting moved nine times
     * arrives as sequence 9 — and calling that a new invitation tells
     * the reader the opposite of what happened.
     */
    @Test
    fun a_resent_request_is_an_update() {
        assertEquals("Updated invite", InviteRules.badge("REQUEST", 9))
        assertEquals("New invite", InviteRules.badge("REQUEST", 0))
        assertEquals("Cancelled", InviteRules.badge("CANCEL", 3))
    }

    /**
     * Offering Yes/No against a PUBLISH or somebody else's REPLY sends
     * an iTIP message to a party that never asked for one.
     */
    @Test
    fun only_a_request_asks_anything() {
        assertTrue(InviteRules.wantsAnswer("REQUEST"))
        assertFalse(InviteRules.wantsAnswer("PUBLISH"))
        assertFalse(InviteRules.wantsAnswer("REPLY"))
        assertFalse(InviteRules.wantsAnswer("CANCEL"))
    }

    /**
     * The instant is the server's, resolved against the invitation's
     * own VTIMEZONE. 16:00 in Santa Clara on 20 August is 23:00 UTC,
     * which is 08:00 the next morning in Tokyo — the number a reader
     * acts on.
     */
    @Test
    fun the_instant_is_read_in_the_readers_zone() {
        val shown = InviteRules.localTime("2026-08-20T23:00:00+00:00", ZoneId.of("Asia/Tokyo"))
        assertTrue("got $shown", shown!!.contains("8:00") || shown.contains("08:00"))
    }

    /** An all-day event has no instant, and inventing one moves it a day. */
    @Test
    fun an_all_day_event_has_no_time() {
        assertNull(InviteRules.localTime(null))
        assertNull(InviteRules.localTime("2026-08-20"))
    }

    /**
     * Exchange writes Windows zone names, which ZoneId does not know,
     * so a reader in Tokyo must not be told the meeting is also at
     * "Tokyo Standard Time" — and must be told when it is Pacific.
     */
    @Test
    fun the_organiser_zone_is_named_only_when_it_differs() {
        val tokyo = ZoneId.of("Asia/Tokyo")
        assertTrue(InviteRules.zoneDiffers("Pacific Standard Time", tokyo))
        assertFalse(InviteRules.zoneDiffers("Tokyo Standard Time", tokyo))
        assertFalse(InviteRules.zoneDiffers("Asia/Tokyo", tokyo))
        assertFalse(InviteRules.zoneDiffers(null, tokyo))
    }

    /**
     * A count answers "how many"; the states answer "is this
     * happening", which is the question somebody deciding whether to go
     * actually has.
     */
    @Test
    fun the_guest_line_says_who_is_coming() {
        val guests = listOf(
            Wire.InviteAttendee(email = "a@x.com", partstat = "ACCEPTED"),
            Wire.InviteAttendee(email = "b@x.com", partstat = "NEEDS-ACTION"),
            Wire.InviteAttendee(email = "c@x.com", partstat = "NEEDS-ACTION"),
        )
        assertEquals("3 guests · 1 yes, 2 awaiting", InviteRules.guests(guests))
        assertEquals("You accepted", InviteRules.answered("ACCEPTED"))
    }
}
