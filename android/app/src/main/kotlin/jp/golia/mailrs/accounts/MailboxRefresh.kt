package jp.golia.mailrs.accounts

/**
 * What a pass learned about messages this device already had.
 *
 * Pure, because the deletion rule is the one that can lose somebody's
 * mail if it is wrong, and it should be checkable without a server.
 */
object MailboxRefresh {
    /**
     * Apply a flag answer to the rows of one folder.
     *
     * Two things happen, and the second is the dangerous one:
     *
     * - a row whose uid came back with a different `\Seen` is updated,
     *   so a message read on a laptop stops being bold here;
     * - **a row whose uid was asked about and did not come back is
     *   removed**, because the server no longer has it.
     *
     * The asking matters: only rows in [asked] may be removed. A row
     * from another folder, or one that was never in the question,
     * cannot be deleted by an answer that was not about it — which is
     * what stops a partial or interrupted fetch from emptying a list.
     */
    fun apply(
        held: List<MailboxRow>,
        accountId: String,
        folder: String,
        asked: Set<Long>,
        answer: Map<Long, Boolean>,
    ): List<MailboxRow> = held.mapNotNull { row ->
        if (row.accountId != accountId || row.folder != folder) return@mapNotNull row
        if (row.uid !in asked) return@mapNotNull row
        val seen = answer[row.uid] ?: return@mapNotNull null
        when (seen) {
            row.seen -> row
            else -> row.copy(seen = seen)
        }
    }
}
