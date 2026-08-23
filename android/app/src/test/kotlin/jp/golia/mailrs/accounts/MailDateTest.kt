package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Reading a `Date:` header.
 *
 * Getting this wrong is quiet: the row shows a plausible time that is
 * hours out, or the list orders itself by nothing at all.
 */
class MailDateTest {
    @Test
    fun `an ordinary date is read`() {
        // 2026-08-05 09:00:00 +0900 == 2026-08-05T00:00:00Z
        assertEquals(
            1_785_888_000L,
            MailDate.epochSeconds("Tue, 5 Aug 2026 09:00:00 +0900"),
        )
    }

    // The day name is optional, and servers disagree about the spaces.
    @Test
    fun `the day name is optional`() {
        val withName = MailDate.epochSeconds("Tue, 5 Aug 2026 09:00:00 +0900")
        assertEquals(withName, MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"))
        assertEquals(withName, MailDate.epochSeconds("Tue,  5  Aug  2026  09:00:00  +0900"))
    }

    // Seconds are optional in RFC 5322.
    @Test
    fun `seconds are optional`() {
        assertEquals(
            MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"),
            MailDate.epochSeconds("5 Aug 2026 09:00 +0900"),
        )
    }

    // The offset is applied, not ignored: the same instant written in
    // two zones is one number.
    @Test
    fun `the zone offset is applied`() {
        assertEquals(
            MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"),
            MailDate.epochSeconds("5 Aug 2026 00:00:00 +0000"),
        )
        assertEquals(
            MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"),
            MailDate.epochSeconds("4 Aug 2026 19:00:00 -0500"),
        )
    }

    // **Obsolete and still in the wild.** Reading `26` as year 26 puts
    // the message two thousand years in the past and sorts the whole
    // list around it.
    @Test
    fun `a two digit year is read as RFC 5322 says`() {
        assertEquals(
            MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"),
            MailDate.epochSeconds("5 Aug 26 09:00:00 +0900"),
        )
        assertEquals(
            MailDate.epochSeconds("5 Aug 1998 09:00:00 +0900"),
            MailDate.epochSeconds("5 Aug 98 09:00:00 +0900"),
        )
    }

    // A trailing comment is legal and common: `+0900 (JST)`.
    @Test
    fun `a trailing comment is ignored`() {
        assertEquals(
            MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"),
            MailDate.epochSeconds("Tue, 5 Aug 2026 09:00:00 +0900 (JST)"),
        )
    }

    // **Null rather than now.** A message with an unreadable date shown
    // as having just arrived jumps to the top of the list and stays
    // there, which is worse than showing no date.
    @Test
    fun `an unreadable date is null rather than now`() {
        assertNull(MailDate.epochSeconds(""))
        assertNull(MailDate.epochSeconds("yesterday"))
        assertNull(MailDate.epochSeconds("5 Xxx 2026 09:00:00 +0900"))
        assertNull(MailDate.epochSeconds("5 Aug 2026 09:00:00"))
    }

    // Guessing UTC for an unknown zone is a silent thirteen-hour error.
    @Test
    fun `an unknown zone is refused rather than guessed as utc`() {
        assertNull(MailDate.epochSeconds("5 Aug 2026 09:00:00 XYZ"))
        assertEquals(
            MailDate.epochSeconds("5 Aug 2026 09:00:00 +0000"),
            MailDate.epochSeconds("5 Aug 2026 09:00:00 GMT"),
        )
    }
}
