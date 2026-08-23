package jp.golia.mailrs.accounts

/**
 * Turning a list of uids into something a server will accept.
 *
 * Two problems, and the second is the one that bites late. A command
 * naming five thousand uids one by one is a line tens of kilobytes
 * long, and servers commonly refuse over-long lines — so the mailbox
 * that most needs its flags refreshed is the one where the refresh
 * silently stops working.
 *
 * Runs are collapsed first because uids in a folder are mostly
 * consecutive, and `12:4000` says the same as four thousand numbers.
 * What is left is chunked, because even collapsed a sparse mailbox can
 * be long.
 */
object UidRanges {
    /** Well under what any server is likely to refuse. */
    const val MAX_CHARS = 900

    /**
     * `1,2,3,7,8,20` becomes `1:3,7:8,20`.
     *
     * Sorted first: a set has no order, and a server reading
     * `20,1:3` gets a valid but pointlessly awkward sequence.
     */
    fun collapse(uids: Collection<Long>): String {
        val sorted = uids.toSortedSet().toList()
        if (sorted.isEmpty()) return ""
        val out = StringBuilder()
        var start = sorted[0]
        var previous = sorted[0]
        fun flush() {
            if (out.isNotEmpty()) out.append(',')
            when (start) {
                previous -> out.append(start)
                else -> out.append(start).append(':').append(previous)
            }
        }
        for (uid in sorted.drop(1)) {
            if (uid == previous + 1) {
                previous = uid
                continue
            }
            flush()
            start = uid
            previous = uid
        }
        flush()
        return out.toString()
    }

    /**
     * The same, split so no one command is too long.
     *
     * Split on **whole runs**, never inside one: half of `1:3` is not
     * a range, and a server would read whatever the halves happen to
     * spell.
     */
    fun batches(uids: Collection<Long>, maxChars: Int = MAX_CHARS): List<String> {
        val whole = collapse(uids)
        if (whole.isEmpty()) return emptyList()
        if (whole.length <= maxChars) return listOf(whole)
        val out = mutableListOf<String>()
        val current = StringBuilder()
        for (run in whole.split(',')) {
            if (current.isNotEmpty() && current.length + 1 + run.length > maxChars) {
                out.add(current.toString())
                current.clear()
            }
            if (current.isNotEmpty()) current.append(',')
            current.append(run)
        }
        if (current.isNotEmpty()) out.add(current.toString())
        return out
    }
}
