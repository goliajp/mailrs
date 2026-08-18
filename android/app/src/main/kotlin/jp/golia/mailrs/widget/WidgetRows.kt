package jp.golia.mailrs.widget

/**
 * How many rows fit a widget of a given height.
 *
 * The widget declares itself resizable in both directions, and drew
 * three rows whatever size it was given: a tall one wasted the space
 * it had been dragged out to fill, and a short one clipped what it
 * promised. Either way the resize handle offers something the widget
 * does not deliver.
 *
 * Pure, so the arithmetic is testable without a launcher.
 */
object WidgetRows {

    /** The heading, plus the widget's own padding, before any row. */
    private const val CHROME_DP = 46

    /** A sender line and a subject line, with the gap above them. */
    private const val ROW_DP = 38

    /**
     * @param heightDp what the launcher offered.
     * @return at least one row wherever there is room for the heading —
     *   a widget too small for even that is a launcher's decision to
     *   make, not a reason to draw nothing.
     */
    fun fitting(heightDp: Int, available: Int): Int {
        if (available <= 0) return 0
        val room = (heightDp - CHROME_DP) / ROW_DP
        return room.coerceIn(1, available)
    }
}
