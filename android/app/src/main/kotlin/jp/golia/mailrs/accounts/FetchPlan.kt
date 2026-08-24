package jp.golia.mailrs.accounts

import kotlinx.serialization.Serializable

/**
 * What this client remembers about a folder between passes.
 */
@Serializable
data class FolderMark(
    /**
     * The validity the uids below were issued under.
     *
     * Kept **with** the uid and never apart: a uid without the
     * validity that issued it is a number that means nothing, and
     * storing them separately is how they drift.
     */
    val uidValidity: Long,
    /** The highest uid read. */
    val highestUid: Long,
    /**
     * The lowest uid read, and where "load earlier" starts from.
     *
     * Travels with the validity for the same reason [highestUid] does:
     * a uid without the validity that issued it is a number that means
     * nothing. Zero means nothing has been read yet.
     *
     * Defaulted, so a mark stored before this field existed still
     * decodes — and defaulted to 0, which reads as "unknown" and makes
     * the first "earlier" tap anchor itself from what is held.
     */
    val lowestUid: Long = 0,
    /**
     * How wide the last "earlier" reach was.
     *
     * Kept because it adapts: a range that came back nearly empty was
     * mostly gaps, and the next one asks wider. Forgetting it makes
     * every tap start narrow again in exactly the mailbox where narrow
     * does not work.
     */
    val earlierSpan: Int = EarlierPlan.FIRST_SPAN,
)

/**
 * What to ask a folder for, given what is already held.
 *
 * Its own type because the decision is where this goes wrong, and it
 * needs no socket to check. Three cases, and the middle one is the one
 * every client gets wrong once.
 */
sealed interface FetchPlan {
    /**
     * Never read this folder: the newest [count] of it.
     *
     * **Not everything.** A first sync of a mailbox with fifty
     * thousand messages would fetch fifty thousand header blocks —
     * hundreds of megabytes, many minutes, and a row list far past
     * what this device stores in one go. Every mail client fetches a
     * window and offers to go further; this fetches the window.
     *
     * By **sequence number**, not uid, because "the last five hundred
     * messages" is what a sequence number means and there is no uid
     * arithmetic that says it — uids have gaps wherever anything was
     * ever deleted.
     */
    data class Newest(val count: Int, val exists: Int) : FetchPlan

    /**
     * Read before, and the server's numbering still means what it
     * meant: only what arrived since.
     */
    data class Since(val uid: Long) : FetchPlan

    /**
     * The server renumbered the folder.
     *
     * **`UIDVALIDITY` changed, so every uid held is meaningless** —
     * uid 4390 is not the message it was, and asking for "everything
     * after 4390" would skip mail or fetch the wrong thing. The folder
     * is read from the start again.
     *
     * Not a fault and not rare: providers renumber after a restore, a
     * migration, or a mailbox rename.
     */
    data class Renumbered(val count: Int, val exists: Int) : FetchPlan

    /**
     * The range, and **which command it belongs to**.
     *
     * `UID FETCH 1:500` and `FETCH 1:500` mean completely different
     * things — the first is uids, the second is positions in the
     * folder — so the two travel together rather than as a string a
     * caller pairs with a verb by hand.
     */
    val range: String
        get() = when (this) {
            is Newest -> window(count, exists)
            is Renumbered -> window(count, exists)
            is Since -> "${uid + 1}:*"
        }

    /** Whether [range] is uids. `false` means sequence numbers. */
    val byUid: Boolean
        get() = when (this) {
            is Since -> true
            else -> false
        }

    companion object {
        /** How much of a folder a first pass reads. */
        const val WINDOW = 500

        /**
         * The last [count] positions, or the whole folder when it is
         * smaller than that.
         */
        internal fun window(count: Int, exists: Int): String {
            val from = maxOf(1, exists - count + 1)
            return "$from:*"
        }

        /**
         * Decide, from what is held and what the server just said.
         *
         * @param exists how many messages the folder holds, from
         *   `SELECT`. Needed because a first pass counts from the end.
         */
        fun decide(
            mark: FolderMark?,
            serverValidity: Long,
            exists: Int = 0,
            window: Int = WINDOW,
        ): FetchPlan = when {
            mark == null -> Newest(window, exists)
            mark.uidValidity != serverValidity -> Renumbered(window, exists)
            mark.highestUid == 0L -> Newest(window, exists)
            else -> Since(mark.highestUid)
        }
    }
}
