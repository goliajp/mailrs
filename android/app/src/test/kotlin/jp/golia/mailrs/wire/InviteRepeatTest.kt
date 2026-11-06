package jp.golia.mailrs.wire

import java.time.ZoneId
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

private val inviteJson = kotlinx.serialization.json.Json {
    ignoreUnknownKeys = true
    explicitNulls = false
}

/**
 * The Teams invitation from the bug report — weekly on Thursday at 22:00
 * Pacific, until 2027-04-29 — as the server serves it, with the next
 * occurrence either side of the US clock change on 1 November.
 * The shapes follow `crates/core-sidestate/tests/recurring_rows.rs`.
 */
private fun payload(next: String?): WireInvite = inviteJson.decodeFromString(
    WireInvite.serializer(),
    """
    {"uid":"golia-sync","sequence":0,"summary":"GOLIA sync","attendees":[],
     "dtstart":{"Zoned":{"tz_name":"Pacific Standard Time","local":"2026-10-29T22:00:00"}},
     "dtstart_utc":"2026-10-30T05:00:00+00:00","dtend_utc":"2026-10-30T06:00:00+00:00",
     "organiser_zone":"America/Los_Angeles",
     "recurrence":{"freq":"WEEKLY","interval":1,"by_day":["TH"],"by_month":[],"by_month_day":[],
                   "count":null,"until":"2027-04-29","plain":true},
     "next_occurrence":${next ?: "null"}}
    """,
)

private const val BEFORE_CHANGE = """{"start":"2026-10-30T05:00:00+00:00","end":"2026-10-30T06:00:00+00:00",
  "organiser_local":"2026-10-29T22:00:00","organiser_offset_secs":-25200}"""
private const val AFTER_CHANGE = """{"start":"2026-11-06T06:00:00+00:00","end":"2026-11-06T07:00:00+00:00",
  "organiser_local":"2026-11-05T22:00:00","organiser_offset_secs":-28800}"""

private val TOKYO = ZoneId.of("Asia/Tokyo")
private val BERLIN = ZoneId.of("Europe/Berlin")

class InviteRepeatTest {
    private var previous: Texts.Lookup? = null

    @Before
    fun englishWords() = TestTexts.install()

    @After
    fun restore() {
        previous?.let { Texts.swap(it) }
        previous = null
    }

    private fun hour(line: String?, h24: String, h12: String) {
        assertTrue("got $line", line!!.contains(h24) || line.contains(h12))
    }

    /** Tokyo has no daylight time: 14:00 before the US change, 15:00 after. */
    @Test
    fun the_next_meeting_in_tokyo_moves_an_hour_when_the_us_clocks_go_back() {
        val before = InviteRepeat.whenLine(payload(BEFORE_CHANGE), TOKYO, Locale.US)
        hour(before, "14:00", "2:00")
        assertTrue("got $before", before!!.startsWith("Next:"))
        assertTrue("got $before", before.endsWith("PDT"))
        val after = InviteRepeat.whenLine(payload(AFTER_CHANGE), TOKYO, Locale.US)
        hour(after, "15:00", "3:00")
        assertTrue("got $after", after!!.endsWith("PST"))
        assertFalse("not a fixed label: $after", after.contains("Pacific Standard Time"))
    }

    /**
     * Berlin goes back a week before the US: in the week between, the
     * meeting is 06:00 there, and 07:00 once both have changed.
     */
    @Test
    fun the_next_meeting_in_berlin_follows_both_changes() {
        hour(InviteRepeat.whenLine(payload(BEFORE_CHANGE), BERLIN, Locale.US), "06:00", "6:00")
        hour(InviteRepeat.whenLine(payload(AFTER_CHANGE), BERLIN, Locale.US), "07:00", "7:00")
    }

    @Test
    fun the_rule_reads_in_the_readers_language() {
        assertEquals(
            "Weekly on Thursday · until Apr 29, 2027 (organizer’s time)",
            InviteRepeat.ruleLine(payload(AFTER_CHANGE), TOKYO, Locale.US),
        )
        previous = TestTexts.swap("values-zh-rCN")
        assertEquals(
            "每周四 · 到 2027年4月29日 为止（按组织者的时间）",
            InviteRepeat.ruleLine(payload(AFTER_CHANGE), TOKYO, Locale.SIMPLIFIED_CHINESE),
        )
    }

    @Test
    fun a_reader_in_the_organisers_zone_is_not_told_it_twice() {
        val la = ZoneId.of("America/Los_Angeles")
        val line = InviteRepeat.whenLine(payload(AFTER_CHANGE), la, Locale.US)
        assertFalse("got $line", line!!.contains("PST"))
        assertEquals("Weekly on Thursday · until Apr 29, 2027", InviteRepeat.ruleLine(payload(AFTER_CHANGE), la, Locale.US))
    }

    @Test
    fun a_series_that_is_over_says_so_and_shows_its_first_meeting() {
        val ended = payload(null)
        assertTrue(InviteRepeat.ruleLine(ended, TOKYO, Locale.US)!!.endsWith("the series has ended"))
        val line = InviteRepeat.whenLine(ended, TOKYO, Locale.US)
        assertFalse("got $line", line!!.startsWith("Next:"))
    }

    @Test
    fun a_single_event_has_no_rule_line() {
        val single = inviteJson.decodeFromString(
            WireInvite.serializer(),
            """{"uid":"x","summary":"x","dtstart_utc":"2026-10-30T05:00:00+00:00","organiser_zone":"America/Los_Angeles"}""",
        )
        assertNull(InviteRepeat.ruleLine(single, TOKYO, Locale.US))
        // 30 October 05:00 UTC is daylight time in Santa Clara
        assertTrue(InviteRepeat.whenLine(single, TOKYO, Locale.US)!!.endsWith("PDT"))
    }
}
