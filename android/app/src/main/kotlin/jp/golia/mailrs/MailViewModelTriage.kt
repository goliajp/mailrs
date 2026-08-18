package jp.golia.mailrs

import jp.golia.mailrs.wire.markAllRead
import kotlinx.coroutines.flow.update
import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.Wire
import kotlinx.coroutines.launch

/**
 * Filing mail: the swipe, the selection, and the undo between them.
 *
 * Extensions rather than methods, for the reason the other two files
 * give — Kotlin has no partial classes and one view model had grown to
 * 1,460 lines. This is the part a person touches most, so it is worth
 * being able to read on its own.
 *
 * The undo window lives here as the whole reason the request is
 * deferred: a row leaves at once and the server is told five seconds
 * later, so undoing costs nothing and swiping four rows costs three
 * requests rather than eight.
 */

/** Long enough to notice the row left, short enough not to feel stuck. */
internal const val UNDO_WINDOW_MS = 5_000L

/**
 * Triage a row: take it off the list now, tell the server later.
 *
 * **The row leaves immediately and the request waits.** Undo is the
 * protection this screen offers instead of a confirmation — the
 * design doc's rule, and Gmail's — so a request sent at the swipe
 * would have to be undone by a second request, and a person who
 * swipes four rows and undoes one would have paid for eight round
 * trips to move three.
 *
 * The pending action is held for [UNDO_WINDOW_MS]. Swiping another
 * row commits the previous one rather than replacing it, because a
 * queue of undoable things is a queue of things a person cannot
 * reason about — one is what a snackbar can honestly offer.
 */
fun MailViewModel.triage(conversation: Wire.Conversation, verb: MailrsClient.Verb) {
    pending?.let { commit(it) }
    val remaining = _state.value.conversations.filterNot { it.threadId == conversation.threadId }
    val action = PendingTriage(conversation, verb, _state.value.conversations)
    pending = action
    _state.update { it.copy(conversations = remaining, undo = action) }

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
 * Android's own pattern for acting on many rows, and the reason the
 * row tap has two meanings — while a selection is on, tapping a row
 * changes the selection rather than opening it, which is what every
 * list on the phone does and what a reader will expect after the
 * first long press.
 */
fun MailViewModel.toggleSelected(threadId: String) {
    val now = _state.value.selected
    val next = if (threadId in now) now - threadId else now + threadId
    _state.update { it.copy(selected = next) }
}

fun MailViewModel.clearSelection() {
    if (_state.value.selected.isEmpty()) return
    _state.update { it.copy(selected = emptySet()) }
}

/**
 * Apply a verb to everything selected, in one request.
 *
 * The rows leave at once and the selection ends, because a
 * selection that survived its own action would invite the same
 * action twice. Unlike a swipe there is no undo window: the
 * snackbar can honestly offer one undo, and a bulk action that
 * silently deferred would leave the list disagreeing with the
 * server for five seconds while the reader watches.
 */
fun MailViewModel.applyToSelection(verb: MailrsClient.Verb) {
    val ids = _state.value.selected.toList()
    if (ids.isEmpty()) return
    val before = _state.value.conversations
    // Read and star leave the rows where they are; the rest take
    // them out of this list. Saying which is which here keeps the
    // list honest about what the server was asked to do.
    val staysInPlace = verb == MailrsClient.Verb.Read || verb == MailrsClient.Verb.Unread ||
        verb == MailrsClient.Verb.Star || verb == MailrsClient.Verb.Unstar
    _state.update { it.copy(
        selected = emptySet(),
        conversations = if (staysInPlace) before else before.filterNot { it.threadId in ids },
    ) }
    viewModelScope.launch {
        when (val r = client.batch(verb, ids)) {
            is MailrsClient.Outcome.Ok -> if (staysInPlace) refresh()
            is MailrsClient.Outcome.Err ->
                // It did not happen, so the rows come back. Mail
                // that vanished on a failed request is mail the
                // person believes they filed and did not.
                _state.update { it.copy(conversations = before, error = r.message) }
        }
    }
}

fun MailViewModel.undo() {
    val action = pending ?: return
    undoToken++
    pending = null
    _state.update { it.copy(conversations = action.before, undo = null) }
}

fun MailViewModel.dismissUndo() {
    _state.update { it.copy(undo = null) }
}

private fun MailViewModel.commit(action: PendingTriage) {
    pending = null
    _state.update { it.copy(undo = null) }
    viewModelScope.launch {
        when (val r = client.batch(action.verb, listOf(action.conversation.threadId))) {
            is MailrsClient.Outcome.Ok -> Unit
            is MailrsClient.Outcome.Err -> {
                // It did not happen, so the row comes back. A row
                // that vanished on a failed request is mail the
                // person believes they filed and did not.
                _state.update { it.copy(
                    conversations = action.before,
                    error = r.message,
                ) }
            }
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
    val verb = if (wanted) MailrsClient.Verb.Star else MailrsClient.Verb.Unstar
    fun withFlag(state: UiState, flagged: Boolean) = state.copy(
        open = state.open?.takeIf { it.threadId == conversation.threadId }?.copy(flagged = flagged)
            ?: state.open,
        conversations = state.conversations.map {
            if (it.threadId == conversation.threadId) it.copy(flagged = flagged) else it
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
 * File the thread being read and go back to the list.
 *
 * The same deferred triage a swipe uses, so the undo snackbar on the
 * list offers it back — reading a message and filing it should cost
 * exactly what swiping past it costs.
 */
fun MailViewModel.triageOpenThread(verb: MailrsClient.Verb) {
    val conversation = _state.value.open ?: return
    closeThread()
    triage(conversation, verb)
}

/**
 * Mark everything in the list being read.
 *
 * The list, not the mailbox: the axes travel with the request, because
 * "mark all as read" pressed inside Notifications should not silence
 * the inbox. Refreshed afterwards rather than adjusted here — the
 * server decides how many it touched, and guessing at counts is how a
 * badge and a list end up disagreeing.
 */
fun MailViewModel.markAllRead() {
    val list = _state.value.list
    viewModelScope.launch {
        when (val r = client.markAllRead(list)) {
            is MailrsClient.Outcome.Ok -> refresh()
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}
