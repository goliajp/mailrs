package jp.golia.mailrs.accounts

/**
 * What to fetch from a POP3 mailbox, and what to remember afterwards.
 *
 * Pure, because the two mistakes available here are both silent. A
 * client that remembers message **numbers** re-downloads the mailbox
 * every time somebody deletes anything, since POP3 renumbers on every
 * session. One that remembers every uidl it has ever seen grows a set
 * that never shrinks, and after a year the bookkeeping is larger than
 * the mailbox.
 */
object Pop3Plan {
    data class Plan(
        /** Message numbers to fetch, oldest first so the list fills in order. */
        val fetch: List<Int>,
        /**
         * What to keep of the old set: the ids still on the server.
         *
         * Ids that have gone are dropped rather than kept — a message
         * deleted on the server cannot come back, and keeping its id
         * forever is how the set outgrows the mailbox.
         */
        val keep: Set<String>,
        /** How many were left for a later pass, so the caller can say so. */
        val deferred: Int,
    )

    /**
     * @param limit how many to fetch in one pass. A first sync of a
     *   mailbox with thousands of messages must not download all of
     *   them before anything appears on screen; the newest are the ones
     *   somebody is looking for.
     */
    fun decide(server: List<Pop3.Uidl>, seen: Set<String>, limit: Int = 200): Plan {
        val present = server.map { it.id }.toSet()
        val unseen = server.filter { it.id !in seen }
        // Newest first to choose, oldest first to fetch: message numbers
        // run in arrival order, so the high ones are the recent ones —
        // and a list that fills from the top reads as mail arriving.
        val chosen = unseen.sortedByDescending { it.number }.take(limit)
        return Plan(
            fetch = chosen.map { it.number }.sorted(),
            keep = seen.intersect(present),
            deferred = unseen.size - chosen.size,
        )
    }
}
