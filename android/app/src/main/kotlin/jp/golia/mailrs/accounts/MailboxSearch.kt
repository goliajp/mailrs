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
        val words = words(query)
        if (words.isEmpty()) return rows
        return rows.filter { row ->
            val haystack = haystack(row)
            words.all { haystack.contains(it) }
        }
    }

    /** A query split into the words every row must match. */
    fun words(query: String) = query.lowercase().split(" ").filter { it.isNotEmpty() }

    /**
     * The text of a row that a search looks in.
     *
     * Here rather than in either caller because the **store keeps a
     * folded copy of it** to search without loading every row, and two
     * spellings of "what a search looks in" is two searches that agree
     * until somebody writes a subject in an alphabet with case.
     * `lowercase` folds all of Unicode; SQLite's `lower` folds ASCII,
     * which is why the folding happens in this language and not in SQL.
     */
    fun haystack(row: MailboxRow) =
        (row.sender + " " + row.subject + " " + row.folder).lowercase()
}
