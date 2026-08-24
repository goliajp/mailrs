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
     *
     * **No production caller since the rows moved into SQLite** — that
     * path uses [decide] and addresses the rows. This is kept because
     * it is the readable statement of the rule *and* because it is
     * implemented in terms of [decide], so its tests are that
     * function's tests. Delete both together or neither.
     */
    fun apply(
        held: List<MailboxRow>,
        accountId: String,
        folder: String,
        asked: Set<Long>,
        answer: Map<Long, Boolean>,
    ): List<MailboxRow> {
        val decision = decide(asked, answer)
        return held.mapNotNull { row ->
            if (row.accountId != accountId || row.folder != folder) return@mapNotNull row
            if (row.uid in decision.gone) return@mapNotNull null
            val seen = decision.flags[row.uid] ?: return@mapNotNull row
            when (seen) {
                row.seen -> row
                else -> row.copy(seen = seen)
            }
        }
    }

    /**
     * The same rule, as a decision rather than as a new list.
     *
     * [apply] rewrites every row it is handed, which is what a store
     * that keeps its rows in one blob needs. A store that can address
     * a row wants to be told *which* rows changed instead, so the rule
     * lives here once and both callers read it from the same place —
     * a second copy of "which uid may be deleted" is the copy that
     * eventually deletes somebody's mail.
     */
    fun decide(asked: Set<Long>, answer: Map<Long, Boolean>) = Decision(
        gone = asked.filterNot { it in answer }.toSet(),
        flags = answer.filterKeys { it in asked },
    )

    /**
     * @property gone uids the server was asked about and did not
     *   acknowledge — and **only** those; see [apply].
     * @property flags the `\Seen` state the server reported.
     */
    data class Decision(val gone: Set<Long>, val flags: Map<Long, Boolean>)
}
