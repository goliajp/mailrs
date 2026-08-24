package jp.golia.mailrs.accounts

/**
 * What to ask for when somebody wants the mail **before** what they
 * have.
 *
 * The first pass takes a window from the end of the folder, so
 * everything older than that window has never been fetched and has no
 * way in. This is the way in.
 *
 * **By uid span, not by position.** A sequence number is what a
 * message is *today*: anything deleted below it shifts it, so a window
 * remembered as "positions 400 to 500" points somewhere else by the
 * next pass. Uids never move. What they do instead is leave **gaps**
 * wherever something was deleted — so a span of 500 uids may hold five
 * messages or five hundred, and neither is a fault.
 *
 * The span therefore adapts: a range that came back nearly empty was
 * mostly holes, and the next one asks wider. A range that came back
 * full asked about the right amount.
 */
object EarlierPlan {
    /** How many uids to reach back over on the first attempt. */
    const val FIRST_SPAN = 200

    /** Beyond this a single fetch is large enough to be its own problem. */
    const val MAX_SPAN = 5_000

    /** How few is few enough to widen. */
    const val THIN = 10

    data class Ask(
        /** The `UID FETCH` range, or null when there is nothing older. */
        val range: String?,
        /** The span this asked about, to be carried to the next answer. */
        val span: Int,
    )

    /**
     * Whether this device is already holding as much as it may.
     *
     * The cap drops the **oldest** rows, and "load earlier" fetches
     * exactly those — so at the ceiling the two undo each other and
     * the button spends a network round trip to change nothing. That
     * is the worst of the three possible behaviours: worse than
     * refusing, because it looks like it worked and did not, and it
     * takes a person several taps to be sure.
     *
     * So this is asked **before** fetching, and a full device says so.
     * It is a real limit, and the honest answer to a real limit is the
     * limit.
     */
    fun atCeiling(held: Int, ceiling: Int = MailboxApply.PER_ACCOUNT) = held >= ceiling

    /**
     * @param lowestHeldUid the smallest uid this device already has for
     *   the folder. **1 means the folder is exhausted**: there is no
     *   uid below it.
     */
    fun decide(lowestHeldUid: Long, span: Int = FIRST_SPAN): Ask {
        if (lowestHeldUid <= 1L) return Ask(null, span)
        val top = lowestHeldUid - 1
        val bottom = maxOf(1L, top - span + 1)
        return Ask("$bottom:$top", span)
    }

    /**
     * The span for the next tap, given what the last one returned.
     *
     * Widened when the answer was thin, because thin means the range
     * was mostly gaps and the same width would be thin again — a
     * person tapping "earlier" five times to see one message would
     * rightly call that broken.
     */
    fun nextSpan(span: Int, returned: Int): Int = when {
        returned >= THIN -> span
        else -> minOf(MAX_SPAN, span * 4)
    }

    /**
     * Whether the answer means the folder is finished.
     *
     * **Not "nothing came back"** — a range that is all gaps returns
     * nothing and there may be plenty below it. It is finished when
     * the range that was asked about reached uid 1.
     */
    fun exhausted(ask: Ask): Boolean = ask.range?.startsWith("1:") == true
}
