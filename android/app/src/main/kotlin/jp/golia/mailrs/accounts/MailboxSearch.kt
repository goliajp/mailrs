package jp.golia.mailrs.accounts

/**
 * Finding a message in what has already been fetched.
 *
 * Local, over the headers on the device — not `IMAP SEARCH`. Two
 * reasons: it answers while somebody is still typing, and it works on
 * a train. What it cannot do is find a message this device has never
 * seen, and the screen says so rather than letting an empty result
 * read as "you have no such mail".
 */
object MailboxSearch {
    /**
     * Rows matching every word of [query].
     *
     * **Every** word, not any: somebody typing two words is narrowing,
     * and a search that widens with each word typed gets further from
     * what they want the more they say.
     *
     * The words may match different fields — "ada lunch" finds a
     * message from Ada about lunch, which is how people search and not
     * how a naive substring match behaves.
     */
    fun matches(rows: List<MailboxRow>, query: String): List<MailboxRow> {
        val words = query.lowercase().split(" ").filter { it.isNotEmpty() }
        if (words.isEmpty()) return rows
        return rows.filter { row ->
            val haystack = (row.sender + " " + row.subject + " " + row.folder).lowercase()
            words.all { haystack.contains(it) }
        }
    }
}
