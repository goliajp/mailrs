package jp.golia.mailrs.wire

import androidx.annotation.StringRes
import jp.golia.mailrs.R

/**
 * What can be done to a conversation, defined once.
 *
 * The same table as the web's `web/src/lib/actions.ts`: each action's
 * words, what it is called once done, the action that takes it back,
 * and whether it asks first. Only deleting for good asks — everything
 * else can be undone, and the undo is the protection.
 *
 * [batch] is the verb `POST /api/conversations/batch` knows, for the
 * actions it carries; the others have a route of their own per
 * conversation.
 */
enum class RowAction(
    @StringRes val label: Int,
    @StringRes val done: Int?,
    val batch: String?,
    /** Whether the row leaves the list it is shown in once this is done. */
    val leavesList: Boolean,
    val confirm: Boolean = false,
) {
    Archive(R.string.action_archive, R.string.action_archive_done, "archive", true),
    Unarchive(R.string.action_unarchive, R.string.action_unarchive_done, "unarchive", true),
    Trash(R.string.action_trash, R.string.action_trash_done, "delete", true),
    Untrash(R.string.action_untrash, R.string.action_untrash_done, "untrash", true),
    DeleteForever(R.string.action_delete_forever, R.string.action_delete_forever_done, null, true, confirm = true),
    MarkJunk(R.string.action_mark_junk, R.string.action_mark_junk_done, null, true),
    MarkNotJunk(R.string.action_mark_not_junk, R.string.action_mark_not_junk_done, null, true),
    MarkNotification(R.string.action_mark_notification, R.string.action_mark_notification_done, null, true),
    MarkPromotion(R.string.action_mark_promotion, R.string.action_mark_promotion_done, null, true),
    MoveToInbox(R.string.action_move_to_inbox, R.string.action_move_to_inbox_done, null, true),
    Pin(R.string.action_pin, R.string.action_pin_done, null, false),
    Unpin(R.string.action_unpin, R.string.action_unpin_done, null, false),
    Snooze(R.string.action_snooze, R.string.action_snooze_done, null, true),
    Unsnooze(R.string.action_unsnooze, R.string.action_unsnooze_done, null, true),
    Read(R.string.action_read, null, "read", false),
    Unread(R.string.action_unread, R.string.action_unread_done, "unread", false),
    Star(R.string.action_star, null, "star", false),
    Unstar(R.string.action_unstar, null, "unstar", false),
    Label(R.string.action_label, null, "label", false),
    Unlabel(R.string.action_unlabel, R.string.action_unlabel_done, "unlabel", true),
    /** Out of Review: it was not fraud. Not in the web's table; the verdict panel offers it there. */
    Release(R.string.action_release, R.string.action_release_done, null, true),
    ;

    /** The action that takes this one back, or null when nothing does. */
    val undo: RowAction?
        get() = when (this) {
            Archive -> Unarchive
            Unarchive -> Archive
            Trash -> Untrash
            Untrash -> Trash
            DeleteForever -> null
            MarkJunk -> MarkNotJunk
            MarkNotJunk -> MarkJunk
            MarkNotification -> MoveToInbox
            MarkPromotion -> MoveToInbox
            MoveToInbox -> null
            Pin -> Unpin
            Unpin -> Pin
            Snooze -> Unsnooze
            Unsnooze -> Snooze
            Read -> Unread
            Unread -> Read
            Star -> Unstar
            Unstar -> Star
            Label -> Unlabel
            Unlabel -> Label
            Release -> null
        }
}

/**
 * Which actions a conversation offers where it is shown, in the web's
 * order (`rowActions` in `actions.ts`), so a person finds them in the
 * same place on every surface.
 */
object RowActions {
    fun forRow(c: Wire.Conversation, list: MailList, nowSeconds: Long): List<RowAction> {
        if (list == MailList.Review) return listOf(RowAction.Release, RowAction.Trash)
        if (c.trashedAt > 0 || list == MailList.Trash) {
            return listOf(pick(c.unreadCount > 0, RowAction.Read, RowAction.Unread), RowAction.Untrash, RowAction.DeleteForever)
        }
        return buildList {
            add(pick(c.unreadCount > 0, RowAction.Read, RowAction.Unread))
            add(pick(c.flagged, RowAction.Unstar, RowAction.Star))
            add(pick(c.pinned, RowAction.Unpin, RowAction.Pin))
            add(pick(c.archived, RowAction.Unarchive, RowAction.Archive))
            add(pick((c.snoozedUntil ?: 0) > nowSeconds, RowAction.Unsnooze, RowAction.Snooze))
            addAll(bucketActions(list))
            add(RowAction.Label)
            if (list == MailList.Label) add(RowAction.Unlabel)
            add(RowAction.Trash)
        }
    }

    /**
     * What a selection of several rows offers: the actions that do not
     * depend on one row's own state, for the list they are in.
     */
    fun forSelection(list: MailList): List<RowAction> = when (list) {
        MailList.Review -> listOf(RowAction.Release, RowAction.Trash)
        MailList.Trash -> listOf(RowAction.Untrash, RowAction.DeleteForever)
        else -> buildList {
            add(RowAction.Unread)
            add(RowAction.Pin)
            add(RowAction.Snooze)
            addAll(bucketActions(list))
            add(RowAction.Label)
            if (list == MailList.Label) add(RowAction.Unlabel)
            add(RowAction.Trash)
        }
    }

    private fun bucketActions(list: MailList): List<RowAction> = when (list) {
        MailList.Junk -> listOf(RowAction.MarkNotJunk)
        MailList.NP -> listOf(RowAction.MoveToInbox, RowAction.MarkJunk)
        else -> listOf(RowAction.MarkNotification, RowAction.MarkPromotion, RowAction.MarkJunk)
    }

    /** Of a pair, the one that applies now. */
    private fun pick(on: Boolean, ifOn: RowAction, ifOff: RowAction): RowAction = when (on) {
        true -> ifOn
        false -> ifOff
    }
}
