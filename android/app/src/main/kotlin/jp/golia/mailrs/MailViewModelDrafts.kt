package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.Wire
import jp.golia.mailrs.wire.deleteDraft
import jp.golia.mailrs.wire.drafts
import jp.golia.mailrs.wire.saveDraft
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Leave the composer, keeping what was written.
 *
 * **Leaving saves.** Every mail client on the phone does, and a
 * half-written message is the one thing a person cannot get back by
 * trying again. An empty composer is not saved, or the drafts list
 * fills with blanks from every mis-tapped compose button.
 */
fun MailViewModel.cancelCompose() {
    val draft = _state.value.composing
    _state.update { it.copy(composing = null, error = null) }
    if (draft == null || draft.isEmpty) return
    viewModelScope.launch {
        val saved = client.saveDraft(
            Wire.SaveDraftRequest(
                id = draft.serverId,
                to = draft.to,
                cc = draft.cc,
                bcc = draft.bcc,
                subject = draft.subject,
                body = draft.body,
                replyToThreadId = draft.replyToThreadId,
            ),
        )
        if (saved is MailrsClient.Outcome.Ok) {
            _state.update { it.copy(draftSaved = true) }
        }
    }
}

fun MailViewModel.draftNoticeShown() {
    _state.update { it.copy(draftSaved = false) }
}

fun MailViewModel.openDrafts() {
    _state.update { it.copy(draftsOpen = true, busy = true, error = null) }
    viewModelScope.launch {
        _state.update {
            when (val r = client.drafts()) {
            is MailrsClient.Outcome.Ok ->
                it.copy(busy = false, drafts = r.value.sortedByDescending { it.updatedAt })
            is MailrsClient.Outcome.Err -> it.copy(busy = false, error = r.message)
        }
        }
    }
}

fun MailViewModel.closeDrafts() {
    _state.update { it.copy(draftsOpen = false) }
}

/**
 * Reopen a saved draft.
 *
 * Its server id travels with it, so saving again updates the same
 * row rather than leaving a copy behind on every edit.
 */
fun MailViewModel.editSavedDraft(d: Wire.Draft) {
    _state.update { it.copy(
        draftsOpen = false,
        composing = Draft(
            id = nextDraftId++,
            to = d.to,
            cc = d.cc,
            bcc = d.bcc,
            subject = d.subject,
            body = d.body,
            replyToThreadId = d.replyToThreadId,
            serverId = d.id,
        ),
    ) }
}

fun MailViewModel.discardDraft(d: Wire.Draft) {
    // Optimistic, and rightly so — a discard that waits for the server
    // feels broken. But the answer has to be read: dropping it showed
    // a draft discarded and left it on the server, to reappear next
    // time the list was opened with no account of where it had been.
    val before = _state.value.drafts
    _state.update { it.copy(drafts = before.filterNot { row -> row.id == d.id }) }
    viewModelScope.launch {
        when (val r = client.deleteDraft(d.id)) {
            is MailrsClient.Outcome.Ok -> Unit
            is MailrsClient.Outcome.Err -> _state.update { it.copy(drafts = before, error = r.message) }
        }
    }
}
