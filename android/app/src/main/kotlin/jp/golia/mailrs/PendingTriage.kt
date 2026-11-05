package jp.golia.mailrs

import jp.golia.mailrs.wire.Wire

/**
 * A triage waiting out its undo window.
 *
 * `before` is the whole list as it was, not just the row: putting
 * one row back at the right index is the same information and more
 * ways to get it wrong.
 */
data class PendingTriage(
    /** The rows acted on — one for a swipe, any number for a selection. */
    val rows: List<Wire.Conversation>,
    val action: jp.golia.mailrs.wire.RowAction,
    val before: List<Wire.Conversation>,
    /**
     * Whether the server has already been told. A swipe waits out the
     * undo window first, so undoing it costs nothing; an action from a
     * menu is sent at once, and undoing it sends the action that takes
     * it back.
     */
    val applied: Boolean = false,
    /** The label a label action put on or took off. */
    val labelId: String? = null,
    /**
     * Set when the undo takes back a block, which is not the action's
     * own undo: `mark-not-junk` would whitelist every sender of the thread.
     */
    val blocked: BlockedSender? = null,
)
