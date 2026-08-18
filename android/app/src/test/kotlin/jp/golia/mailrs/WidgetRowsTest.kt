package jp.golia.mailrs

import jp.golia.mailrs.widget.WidgetRows
import org.junit.Assert.assertEquals
import org.junit.Test

class WidgetRowsTest {

    @Test
    fun `a taller widget shows more`() {
        // The manifest's own minimum is 110dp: one row.
        assertEquals(1, WidgetRows.fitting(heightDp = 110, available = 9))
        assertEquals(3, WidgetRows.fitting(heightDp = 160, available = 9))
        assertEquals(5, WidgetRows.fitting(heightDp = 240, available = 9))
    }

    @Test
    fun `never more than there is mail`() {
        assertEquals(2, WidgetRows.fitting(heightDp = 400, available = 2))
        assertEquals(0, WidgetRows.fitting(heightDp = 400, available = 0))
    }

    @Test
    fun `a widget squeezed below one row still draws one`() {
        // Better a clipped row than a heading over blank space: the
        // launcher decided this size, and the widget's job is to say
        // what it can.
        assertEquals(1, WidgetRows.fitting(heightDp = 40, available = 9))
    }
}
