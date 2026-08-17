package jp.golia.mailrs.wire

/**
 * Paging the conversation list.
 *
 * Ported from `ios/Mailrs/Wire/ThreadPage.swift`, hazard and all.
 *
 * `GET /api/conversations` pages by keyset, not by cursor: `before_ts`
 * plus `limit`, applied as `latest_date < before_ts`. There is no
 * `has_more` and no cursor to follow.
 *
 * Which leaves one trap, and it is not hypothetical: `last_date` is
 * whole seconds, so several threads share one. Asking for
 * `before_ts = oldest.lastDate` drops every sibling of that second that
 * did not fit on the page — **silently**, because a shorter list looks
 * exactly like the end of the mailbox. `kevy-patterns.md` measured 929
 * such collisions over 30k rows on this data.
 *
 * So the next page asks for `oldest.lastDate + 1`, deliberately
 * re-requesting the boundary second, and [merge] drops what is already
 * held. The overlap costs a few rows; the alternative loses mail.
 */
object ThreadPage {

    /** The `before_ts` that will not skip the boundary second. */
    fun nextBefore(rows: List<Wire.Conversation>): Long? = rows.lastOrNull()?.lastDate?.plus(1)

    data class Merged(
        val rows: List<Wire.Conversation>,
        /**
         * Whether the page carried anything not already held.
         *
         * The termination condition, and it has to be this rather than a
         * count: re-requesting the boundary second means a page can come
         * back full of rows already on screen, and stopping on "was it a
         * full page?" would ask for the same second forever. A page with
         * nothing new is the end.
         */
        val progressed: Boolean,
    )

    fun merge(existing: List<Wire.Conversation>, incoming: List<Wire.Conversation>): Merged {
        val seen = existing.mapTo(HashSet()) { it.threadId }
        val rows = existing.toMutableList()
        var progressed = false
        for (row in incoming) {
            if (!seen.add(row.threadId)) continue
            rows += row
            progressed = true
        }
        return Merged(rows, progressed)
    }
}
