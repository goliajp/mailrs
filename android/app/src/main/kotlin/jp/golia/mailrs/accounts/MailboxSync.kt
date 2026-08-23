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
        /**
         * Per folder, the flags the server reports for uids this
         * device already held — and, by their absence from it, which
         * of those uids the server no longer has.
         */
        val refreshed: Map<String, Map<Long, Boolean>> = emptyMap(),
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
        /**
         * What this device already holds, per folder. Asked about
         * again each pass so a message read or deleted on another
         * device stops being wrong here — see [MailboxRefresh].
         */
        held: Map<String, List<Long>> = emptyMap(),
    ): Result {
        val rows = mutableListOf<MailboxRow>()
        val out = marks.toMutableMap()
        val renumbered = mutableSetOf<String>()
        val refreshed = mutableMapOf<String, Map<Long, Boolean>>()

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

                // And what happened to the ones already here. Cheap —
                // flags only — and the only way this device notices a
                // message read on a laptop or deleted from a phone.
                // Skipped on a renumbering, where the old uids mean
                // nothing and every row for the folder is replaced.
                val already = held[folder.name].orEmpty()
                if (plan !is FetchPlan.Renumbered && already.isNotEmpty()) {
                    refreshed[folder.name] = session.flags(already)
                }
            } catch (e: Exception) {
                continue
            }
        }
        return Result(rows, out, renumbered, refreshed)
    }

    /**
     * Whether a folder belongs in a merged **inbox**.
     *
     * `\Noselect` cannot be opened at all — it is a node in the tree
     * rather than a mailbox. A provider's view holding a copy of
     * everything doubles every message, and its Trash and Spam are the
     * two a person would skip themselves.
     *
     * **Sent and Drafts are skipped too**, and that is a decision
     * rather than an omission: this list is what arrived. A draft is
     * not a message at all — it has not been sent to anybody — and a
     * copy of everything the person wrote, interleaved by date with
     * what they received, is what every "all inboxes" view in every
     * mail client deliberately does not show.
     *
     * A server with no special-use markers is read whole, which is the
     * right default: a folder nobody has labelled is a folder somebody
     * made, and those are where filed mail lives.
     */
    fun worthReading(name: String, attributes: List<String>, skip: List<String>): Boolean {
        val upper = attributes.map { it.uppercase() }.toSet()
        if ("\\NOSELECT" in upper) return false
        val notAnInbox = setOf("\\ALL", "\\TRASH", "\\JUNK", "\\SENT", "\\DRAFTS")
        if (upper.any { it in notAnInbox }) return false
        return skip.none { it.equals(name, ignoreCase = true) }
    }
}
