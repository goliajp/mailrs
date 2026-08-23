package jp.golia.mailrs.accounts

/**
 * One pass over one account.
 *
 * The orchestration, kept apart from the socket and from the rules it
 * applies: what to ask for is [FetchPlan], what to keep is
 * [MailboxApply], and this is the order they happen in.
 */
object MailboxSync {
    /** What one pass produced. */
    data class Result(
        val rows: List<MailboxRow>,
        val marks: Map<String, FolderMark>,
        /**
         * Folders whose numbering changed, so their held rows are
         * worthless and must be replaced rather than merged.
         */
        val renumbered: Set<String>,
    )

    /**
     * Read the folders worth reading, and say what to keep.
     *
     * Failures of **one folder do not fail the pass**: a mailbox with
     * twelve folders where one is broken should show the other eleven,
     * and a person who cannot see any mail because of a folder they
     * never open has no way to work that out.
     */
    suspend fun pass(
        account: MailAccount,
        session: ImapSession,
        marks: Map<String, FolderMark>,
    ): Result {
        val rows = mutableListOf<MailboxRow>()
        val out = marks.toMutableMap()
        val renumbered = mutableSetOf<String>()

        for (folder in session.list()) {
            if (!worthReading(folder.name, folder.attributes, account.skipFolders)) continue
            try {
                val (validity, _) = session.select(folder.name)
                val plan = FetchPlan.decide(marks[folder.name], validity)
                if (plan is FetchPlan.Renumbered) renumbered += folder.name

                val fetched = session.fetchHeaders(plan.range)
                var highest = if (plan is FetchPlan.Renumbered) {
                    0L
                } else {
                    marks[folder.name]?.highestUid ?: 0L
                }
                for (message in fetched) {
                    rows += MailboxRow(
                        accountId = account.id,
                        uid = message.uid,
                        folder = folder.name,
                        seen = message.seen,
                        sender = MessageHeaders.senderName(message.headers.from),
                        subject = message.headers.subject,
                        date = message.date,
                        messageId = message.headers.messageId,
                    )
                    highest = maxOf(highest, message.uid)
                }
                // Written **after** the rows are in hand, not before: a
                // mark saved for messages that were never kept skips
                // them for good, and nothing afterwards would ask for
                // them again.
                out[folder.name] = FolderMark(validity, highest)
            } catch (e: Exception) {
                continue
            }
        }
        return Result(rows, out, renumbered)
    }

    /**
     * Whether a folder is worth reading.
     *
     * `\Noselect` cannot be opened at all — it is a node in the tree
     * rather than a mailbox. A provider's view holding a copy of
     * everything doubles every message, and its Trash and Spam are the
     * two a person would skip themselves.
     */
    fun worthReading(name: String, attributes: List<String>, skip: List<String>): Boolean {
        val upper = attributes.map { it.uppercase() }.toSet()
        if ("\\NOSELECT" in upper) return false
        if ("\\ALL" in upper || "\\TRASH" in upper || "\\JUNK" in upper) return false
        return skip.none { it.equals(name, ignoreCase = true) }
    }
}
