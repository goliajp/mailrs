package jp.golia.mailrs

import jp.golia.mailrs.wire.redraft
import jp.golia.mailrs.wire.resend
import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.SendJoin
import jp.golia.mailrs.wire.Wire
import jp.golia.mailrs.wire.cancelScheduled
import jp.golia.mailrs.wire.scheduledSends
import jp.golia.mailrs.wire.sends
import jp.golia.mailrs.wire.sentMessages
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * What was sent.
 *
 * Two requests, because the answer is two things: what the maildir
 * sweep has filed and what the delivery projection knows. A failure of
 * either is not a failure of the screen — a list with no statuses is
 * still the list, and no statuses is what every mailbox older than the
 * projection looks like anyway.
 */
fun MailViewModel.openSent() {
    _state.update { it.copy(sentOpen = true, busy = true, error = null) }
    viewModelScope.launch {
        val messages = (client.sentMessages() as? MailrsClient.Outcome.Ok)?.value.orEmpty()
        val sends = (client.sends() as? MailrsClient.Outcome.Ok)?.value.orEmpty()
        val waiting = (client.scheduledSends() as? MailrsClient.Outcome.Ok)?.value.orEmpty()
        _state.update { it.copy(
            busy = false,
            sentMail = SendJoin.join(messages, sends),
            scheduled = waiting.sortedBy { s -> s.scheduledAt },
        ) }
    }
}

fun MailViewModel.closeSent() {
    _state.update { it.copy(sentOpen = false) }
}

/**
 * Call a scheduled message back before it leaves.
 *
 * The row goes at once and the list is re-read afterwards: an undo has
 * nothing to undo here — the message is still a draft on the server
 * either way — and a row that lingers while the request travels reads
 * as a cancel that did not work.
 */
fun MailViewModel.cancelScheduled(send: Wire.ScheduledSend) {
    _state.update { it.copy(scheduled = it.scheduled.filterNot { s -> s.id == send.id }) }
    viewModelScope.launch {
        when (val r = client.cancelScheduled(send.id)) {
            is MailrsClient.Outcome.Ok -> openSent()
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}

/**
 * Send it again, byte for byte.
 *
 * The list is re-read afterwards rather than adjusted here: a resend
 * makes a *new* row with its own status, and guessing at that shape
 * would put a second line on screen that the server never agreed to.
 */
fun MailViewModel.resend(row: SendJoin.Row) {
    val id = row.sendId ?: return
    viewModelScope.launch {
        when (val r = client.resend(id)) {
            is MailrsClient.Outcome.Ok -> openSent()
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}

/**
 * Open a sent message for editing, and send it again changed.
 *
 * The other half of resend, and the half that fixes anything: a resend
 * re-enqueues the stored bytes **unchanged**, so a message that failed
 * because the address was wrong fails again. This one comes back as a
 * draft.
 *
 * Its attachments are carried rather than fetched — the server holds
 * the bytes and the send names which to keep by index.
 */
fun MailViewModel.redraft(row: SendJoin.Row) {
    val id = row.sendId ?: return
    viewModelScope.launch {
        when (val r = client.redraft(id)) {
            is MailrsClient.Outcome.Ok -> _state.update {
                it.copy(
                    sentOpen = false,
                    composing = Draft(
                        id = nextDraftId++,
                        to = r.value.to.joinToString(", "),
                        cc = r.value.cc.joinToString(", "),
                        bcc = r.value.bcc.joinToString(", "),
                        subject = r.value.subject,
                        body = r.value.body,
                        inReplyTo = r.value.inReplyTo,
                        redraftOf = r.value.redraftOf,
                        carried = r.value.attachments,
                    ),
                    error = null,
                )
            }
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}
