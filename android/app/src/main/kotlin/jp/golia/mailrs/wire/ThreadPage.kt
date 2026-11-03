package jp.golia.mailrs.wire

import java.net.URLEncoder

/**
 * Paging the conversation list, as `ios/Mailrs/Wire/ThreadPage.swift` does.
 *
 * Every row of `GET /api/conversations` (and of `GET /api/quarantine`)
 * carries a `cursor`, and the next page is asked for with the last row's
 * one as `cursor=`. The server orders rows by time and, within one
 * second, by a key of its own, and starts the next page strictly after
 * the cursor's row: a second that spans two pages loses nothing and
 * repeats nothing.
 *
 * What it replaced: `before_ts`, which compares whole seconds. Several
 * threads share a second (production measured 929 such collisions over
 * 30k rows), so asking for the oldest row's own second dropped the
 * siblings that did not fit, and asking for one second past it — what
 * this did until the cursor — re-requested the boundary second, which
 * only works while that second holds fewer rows than a page. With more,
 * the next page came back as the same rows, read as the end, and the
 * rest of that second was never shown.
 *
 * A row without a cursor still pages the old way, which the server keeps
 * for clients not yet moved. [merge] drops what is already held either
 * way.
 */
object ThreadPage {

    /** Where the page after a list of rows starts. */
    sealed interface Next {
        data class Cursor(val value: String) : Next

        /** The keyset of old: one second past the oldest row. */
        data class Before(val seconds: Long) : Next
    }

    fun next(rows: List<Wire.Conversation>): Next? {
        val oldest = rows.lastOrNull() ?: return null
        val cursor = oldest.cursor ?: return Next.Before(oldest.lastDate + 1)
        return Next.Cursor(cursor)
    }

    /** The query parameter that asks for [next], with its leading `&`, or nothing. */
    fun query(next: Next?): String = when (next) {
        // the String-named overload: the Charset one is API 33
        is Next.Cursor -> "&cursor=" + URLEncoder.encode(next.value, "UTF-8")
        is Next.Before -> "&before_ts=${next.seconds}"
        null -> ""
    }

    data class Merged(
        val rows: List<Wire.Conversation>,
        /**
         * Whether the page carried anything not already held.
         *
         * The termination condition: a page with nothing new is the end.
         * It has to be this rather than "was the page full?", because a
         * page asked for the old way re-requests the boundary second and
         * can come back full of rows already on screen.
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
