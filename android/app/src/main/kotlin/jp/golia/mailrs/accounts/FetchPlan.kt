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
)

/**
 * What to ask a folder for, given what is already held.
 *
 * Its own type because the decision is where this goes wrong, and it
 * needs no socket to check. Three cases, and the middle one is the one
 * every client gets wrong once.
 */
sealed interface FetchPlan {
    /** Never read this folder: everything in it. */
    data object Everything : FetchPlan

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
    data object Renumbered : FetchPlan

    /** The range for a `UID FETCH`. */
    val range: String
        get() = when (this) {
            is Everything, is Renumbered -> "1:*"
            is Since -> "${uid + 1}:*"
        }

    companion object {
        /** Decide, from what is held and what the server just said. */
        fun decide(mark: FolderMark?, serverValidity: Long): FetchPlan = when {
            mark == null -> Everything
            mark.uidValidity != serverValidity -> Renumbered
            mark.highestUid == 0L -> Everything
            else -> Since(mark.highestUid)
        }
    }
}
