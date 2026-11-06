package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailList
import jp.golia.mailrs.wire.MailListAxes
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.RowAction
import jp.golia.mailrs.wire.Wire
import jp.golia.mailrs.wire.act
import jp.golia.mailrs.wire.markAllRead
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Filing mail: the swipe, the selection, the menus, and the undo between
 * them.
 *
 * Extensions rather than methods, for the reason the other files give —
 * Kotlin has no partial classes and one view model had grown to 1,460
 * lines. This is the part a person touches most, so it is worth being
 * able to read on its own.
 *
 * Two kinds of undo, one snackbar. A swipe holds its request for the
 * undo window, so taking it back costs nothing; an action chosen from a
 * menu is sent at once, as the web does, and taking it back sends the
 * action that undoes it (the table in [RowAction]).
 */

/** Long enough to notice the row left, short enough not to feel stuck. */
internal const val UNDO_WINDOW_MS = 5_000L

/** The axes the list showing asks for: a label's own, or the list's. */
fun UiState.axes(): MailListAxes = when {
    label != null -> MailList.labelAxes(label.id)
    list == MailList.Inbox -> list.axes.copy(section = inboxSection)
    else -> list.axes
}

/**
 * Triage a row: take it off the list now, tell the server later.
 *
 * **The row leaves immediately and the request waits.** Undo is the
 * protection this screen offers instead of a confirmation — the design
 * doc's rule, and Gmail's — so a request sent at the swipe would have to
 * be undone by a second request, and a person who swipes four rows and
 * undoes one would have paid for eight round trips to move three.
 *
 * The pending action is held for [UNDO_WINDOW_MS]. Swiping another row
 * commits the previous one rather than replacing it, because a queue of
 * undoable things is a queue of things a person cannot reason about —
 * one is what a snackbar can honestly offer.
 */
fun MailViewModel.triage(conversation: Wire.Conversation, action: RowAction) {
    // Read and unread change the row where it is; with nothing leaving
    // the list there is nothing to hold back, so it is sent now, as a menu
    // sends it, and the snackbar's undo sends the opposite.
    if (!action.leavesList) {
        actNow(listOf(conversation), action, null)
        return
    }
    pending?.let { commit(it) }
    val before = _state.value.conversations
    val remaining = before.filterNot { it.threadId == conversation.threadId }
    val held = PendingTriage(listOf(conversation), action, before)
    pending = held
    _state.update { it.copy(conversations = remaining, undo = held) }

    val token = ++undoToken
    viewModelScope.launch {
        kotlinx.coroutines.delay(UNDO_WINDOW_MS)
        // A later swipe, or an undo, moved on without us.
        if (token != undoToken) return@launch
        pending?.let { commit(it) }
    }
}

/**
 * Selection mode: long-press starts it, tapping adds and removes.
 *
 * Android's own pattern for acting on many rows, and the reason the row
 * tap has two meanings — while a selection is on, tapping a row changes
 * the selection rather than opening it, which is what every list on the
 * phone does and what a reader will expect after the first long press.
 */
fun MailViewModel.toggleSelected(threadId: String) {
    val now = _state.value.selected
    val next = when (threadId in now) {
        true -> now - threadId
        false -> now + threadId
    }
    _state.update { it.copy(selected = next) }
}

fun MailViewModel.clearSelection() {
    if (_state.value.selected.isEmpty()) return
    _state.update { it.copy(selected = emptySet()) }
}

/**
 * Apply an action to everything selected.
 *
 * The rows change at once and the selection ends, because a selection
 * that survived its own action would invite the same action twice.
 */
fun MailViewModel.applyToSelection(action: RowAction, labelId: String? = null, until: Long? = null) {
    val ids = _state.value.selected
    val rows = _state.value.conversations.filter { it.threadId in ids }
    if (rows.isEmpty()) return
    _state.update { it.copy(selected = emptySet()) }
    actNow(rows, action, labelId, until)
}

/** Apply an action from the open thread's menu, and go back to the list when it leaves. */
fun MailViewModel.applyToOpenThread(action: RowAction, labelId: String? = null, until: Long? = null) {
    val conversation = _state.value.open ?: return
    if (action.leavesList) closeThread()
    actNow(listOf(conversation), action, labelId, until)
}

/**
 * Ask before deleting for good: it is the one action nothing takes back.
 * The rows wait in [UiState.confirmForever] until the reader answers.
 */
fun MailViewModel.askDeleteForever(threadIds: List<String>) {
    _state.update { it.copy(confirmForever = threadIds, selected = emptySet()) }
}

fun MailViewModel.answerDeleteForever(yes: Boolean) {
    val ids = _state.value.confirmForever
    _state.update { it.copy(confirmForever = emptyList()) }
    if (!yes || ids.isEmpty()) return
    _state.value.open?.let { open -> if (open.threadId in ids) closeThread() }
    val rows = _state.value.conversations.filter { it.threadId in ids }
    actNow(rows, RowAction.DeleteForever, null)
}

/**
 * Send [action] now and show it on the list, with the undo it has.
 *
 * A failure puts the rows back: mail that vanished on a failed request
 * is mail the person believes they filed and did not.
 */
internal fun MailViewModel.actNow(
    rows: List<Wire.Conversation>,
    action: RowAction,
    labelId: String?,
    until: Long? = null,
) {
    pending?.let { commit(it) }
    val before = _state.value.conversations
    val ids = rows.map { it.threadId }.toSet()
    val after = when (action.leavesList) {
        true -> before.filterNot { it.threadId in ids }
        false -> before.map { row -> changedBy(row, action, ids, labelId) }
    }
    val done = PendingTriage(rows, action, before, applied = true, labelId = labelId, until = until)
    _state.update { it.copy(conversations = after, undo = done) }
    viewModelScope.launch {
        // Deleting for good wants the password typed again on the server,
        // as on the web; the prompt asks and the request is sent once more.
        when (val r = confirmed { client.act(action, ids.toList(), labelId, until) }) {
            is MailrsClient.Outcome.Ok -> Unit
            is MailrsClient.Outcome.Err -> _state.update { it.copy(conversations = before, undo = null, error = r.message) }
        }
    }
}

/** A row as an action that keeps it in the list leaves it. */
private fun changedBy(row: Wire.Conversation, action: RowAction, ids: Set<String>, labelId: String?): Wire.Conversation {
    if (row.threadId !in ids) return row
    return when (action) {
        RowAction.Read -> row.copy(unreadCount = 0)
        RowAction.Unread -> row.copy(unreadCount = maxOf(1, row.unreadCount))
        RowAction.Star -> row.copy(flagged = true)
        RowAction.Unstar -> row.copy(flagged = false)
        RowAction.Pin -> row.copy(pinned = true)
        RowAction.Unpin -> row.copy(pinned = false)
        RowAction.Label -> row.copy(labels = (row.labels + listOfNotNull(labelId)).distinct())
        else -> row
    }
}

fun MailViewModel.undo() {
    val action = pending ?: _state.value.undo ?: return
    undoToken++
    pending = null
    _state.update { it.copy(conversations = action.before, undo = null) }
    action.blocked?.let { undoBlock(it); return }
    if (!action.applied) return
    val back = action.action.undo ?: return
    viewModelScope.launch {
        // waking a row is taken back by snoozing it to the time it had
        val r = client.act(back, action.rows.map { it.threadId }, action.labelId, action.rows.first().snoozedUntil)
        if (r is MailrsClient.Outcome.Err) {
            // It did not go back, so the list should not say it did.
            _state.update { it.copy(error = r.message) }
            refresh()
        }
    }
}

fun MailViewModel.dismissUndo() {
    _state.update { it.copy(undo = null) }
}

private fun MailViewModel.commit(action: PendingTriage) {
    pending = null
    _state.update { it.copy(undo = null) }
    viewModelScope.launch {
        when (val r = client.act(action.action, action.rows.map { it.threadId }, action.labelId, action.until)) {
            is MailrsClient.Outcome.Ok -> Unit
            // It did not happen, so the row comes back. A row that
            // vanished on a failed request is mail the person believes
            // they filed and did not.
            is MailrsClient.Outcome.Err -> _state.update { it.copy(conversations = action.before, error = r.message) }
        }
    }
}

/**
 * Star or unstar the thread being read.
 *
 * The row changes here and now, and the server is told at once — no
 * undo window, unlike the swipe. A star is not destructive and nothing
 * leaves the list, so deferring it would mean the icon and the server
 * disagreed for five seconds over something the person can simply tap
 * again.
 */
fun MailViewModel.toggleStar(conversation: Wire.Conversation) {
    val wanted = !conversation.flagged
    val verb = when (wanted) {
        true -> MailrsClient.Verb.Star
        false -> MailrsClient.Verb.Unstar
    }
    fun withFlag(state: UiState, flagged: Boolean) = state.copy(
        open = state.open?.takeIf { it.threadId == conversation.threadId }?.copy(flagged = flagged)
            ?: state.open,
        conversations = state.conversations.map { row ->
            when (row.threadId == conversation.threadId) {
                true -> row.copy(flagged = flagged)
                false -> row
            }
        },
    )
    _state.update { withFlag(it, wanted) }
    viewModelScope.launch {
        if (client.batch(verb, listOf(conversation.threadId)) is MailrsClient.Outcome.Err) {
            // It did not happen. A star that stayed lit on a failed
            // request is the person believing the thread is kept.
            _state.update { withFlag(it, conversation.flagged) }
        }
    }
}

/**
 * Mark the thread being read unread and go back to the list.
 *
 * Not a triage: the thread stays in the list it was opened from, now
 * unread, so it changes in place and the server is told at once, as a
 * star is. Filed with the triage verbs it left the inbox until the next
 * refresh, which reads as the mail having been archived.
 */
fun MailViewModel.markOpenThreadUnread() {
    val conversation = _state.value.open ?: return
    closeThread()
    fun withUnread(state: UiState, unread: Int) = state.copy(
        conversations = state.conversations.map { row ->
            when (row.threadId == conversation.threadId) {
                true -> row.copy(unreadCount = unread)
                false -> row
            }
        },
    )
    _state.update { withUnread(it, maxOf(1, conversation.unreadCount)) }
    viewModelScope.launch {
        if (client.batch(MailrsClient.Verb.Unread, listOf(conversation.threadId)) is MailrsClient.Outcome.Err) {
            _state.update { withUnread(it, conversation.unreadCount) }
        }
    }
}

/**
 * File the thread being read and go back to the list.
 *
 * The same deferred triage a swipe uses, so the undo snackbar on the
 * list offers it back — reading a message and filing it should cost
 * exactly what swiping past it costs.
 */
fun MailViewModel.triageOpenThread(action: RowAction) {
    val conversation = _state.value.open ?: return
    closeThread()
    triage(conversation, action)
}

/**
 * Mark everything in the list being read.
 *
 * The list, not the mailbox: the axes travel with the request, because
 * "mark all as read" pressed inside Notifications should not silence the
 * inbox. Refreshed afterwards rather than adjusted here — the server
 * decides how many it touched, and guessing at counts is how a badge and
 * a list end up disagreeing.
 */
fun MailViewModel.markAllRead() {
    val axes = _state.value.axes()
    viewModelScope.launch {
        when (val r = client.markAllRead(axes)) {
            is MailrsClient.Outcome.Ok -> refresh()
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}
