package jp.golia.mailrs.wire

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DateChipTest {
    private fun s(date: String, datetime: String? = null, text: String = "written") =
        Wire.DateSuggestion(date = date, datetime = datetime, text = text)

    /**
     * The row from the 2026-08-21 report: three days written three
     * ways. What must not differ afterwards is the pattern.
     */
    @Test
    fun `three formats become one shape`() {
        val labels =
            listOf(
                    s("2026-08-21", text = "Aug 21 2026"),
                    s("2026-08-20", text = "2026-08-20"),
                    s("2026-08-19", text = "2026-08-19"),
                )
                .map { DateChip.label(it, Locale.UK) }
        val shapes = labels.map { it.replace(Regex("\\d+"), "#") }.toSet()
        assertEquals(1, shapes.size)
        assertEquals(3, labels.toSet().size)
    }

    /** A bare date read as an instant would render as the day before. */
    @Test
    fun `the day is the local day`() {
        assertTrue(DateChip.label(s("2026-08-21"), Locale.UK).contains("21"))
    }

    @Test
    fun `an hour shows only when one was written`() {
        val withTime = DateChip.label(s("2026-08-25", "2026-08-25T14:00:00"), Locale.UK)
        val allDay = DateChip.label(s("2026-08-25"), Locale.UK)
        assertNotEquals(withTime, allDay)
        assertTrue(withTime.length > allDay.length)
    }

    /** Unparseable input falls back rather than inventing a day. */
    @Test
    fun `nonsense falls back`() {
        // No date at all: the writer's words are all there is.
        assertEquals("written", DateChip.label(s("not-a-date"), Locale.UK))
        // A date but an unreadable clock: the day, and no invented hour.
        assertEquals(
            DateChip.label(s("2026-08-25"), Locale.UK),
            DateChip.label(s("2026-08-25", "garbage"), Locale.UK),
        )
    }
}
