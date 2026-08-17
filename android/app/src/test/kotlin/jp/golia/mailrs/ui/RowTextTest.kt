package jp.golia.mailrs.ui

import jp.golia.mailrs.wire.Wire
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The two bits of prose the lists compute rather than read.
 *
 * Both are the kind of thing that regresses quietly: nothing crashes
 * when a size reads "0 KB" or a draft row says "Empty draft" for a
 * message that has a recipient.
 */
class RowTextTest {

    /**
     * Decimal units, because that is what every file manager on the
     * phone shows — a mail client disagreeing with the file manager
     * about the same file is a small lie repeated often.
     */
    @Test
    fun a_size_reads_the_way_the_phone_reads_it() {
        assertEquals("0 B", humanSize(0))
        assertEquals("999 B", humanSize(999))
        assertEquals("1 KB", humanSize(1_000))
        assertEquals("1.2 MB", humanSize(1_234_567))
        assertEquals("1.2 GB", humanSize(1_234_567_890))
    }

    /** Subject first — it is what the message is about. */
    @Test
    fun a_draft_row_leads_with_its_subject() {
        val d = Wire.Draft(id = 1, to = "a@x.test", subject = "Lunch", body = "one")
        assertEquals("Lunch", headline(d))
    }

    /**
     * A subject is often the last thing written and often blank, so the
     * recipient carries the row when it is. A list of "(no subject)"
     * tells a reader nothing about which one to open.
     */
    @Test
    fun a_draft_without_a_subject_names_who_it_is_to() {
        val d = Wire.Draft(id = 1, to = "a@x.test", subject = "   ", body = "one")
        assertEquals("To a@x.test", headline(d))
    }

    /** With neither, the first line that has anything on it. */
    @Test
    fun a_draft_with_only_a_body_shows_how_it_starts() {
        val d = Wire.Draft(id = 1, body = "\n\n  the actual first line\nsecond")
        assertEquals("the actual first line", headline(d))
    }

    @Test
    fun a_draft_with_nothing_at_all_says_so() {
        assertEquals("Empty draft", headline(Wire.Draft(id = 1)))
    }
}
