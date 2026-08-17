package jp.golia.mailrs.ui

/**
 * Whether there is room to show the list and a message at once.
 *
 * A phone shows one screen at a time and a tablet — or a foldable that
 * has been opened — should not: on 600dp of width, a conversation list
 * occupying the whole display while the message it was opened from is
 * nowhere is a phone layout stretched, which is the thing Android's
 * large-screen guidance is about.
 *
 * The threshold is Material's own **medium** breakpoint, 600dp, which
 * is where a second pane starts to be worth having rather than a
 * cramped column beside a cramped column.
 *
 * A pure function so the decision can be read and tested without a
 * tablet: the layout that follows from it is a `Row`, and the rule is
 * the part worth being sure about.
 */
object Panes {

    const val MEDIUM_WIDTH_DP = 600

    /** How wide the list pane is when there are two of them. */
    const val LIST_PANE_WIDTH_DP = 360

    fun twoPanes(widthDp: Int): Boolean = widthDp >= MEDIUM_WIDTH_DP
}
