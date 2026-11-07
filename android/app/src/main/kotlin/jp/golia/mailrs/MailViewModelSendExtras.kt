package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailSignature
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.MarkdownHtml
import jp.golia.mailrs.wire.SendAddresses
import jp.golia.mailrs.wire.Template
import jp.golia.mailrs.wire.TemplateApply
import jp.golia.mailrs.wire.cancelPendingSend
import jp.golia.mailrs.wire.role
import jp.golia.mailrs.wire.templates
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** A send that landed: its id, which is what Undo names, and how Undo went. */
data class SentNote(val messageId: String, val undo: Undo = Undo.Offered) {
    enum class Undo { Offered, Cancelling, Cancelled, TooLate }
}

/**
 * The body as sent — signed on the way out, not in the composer, so what was
 * typed is what is shown — and its `html_body` when the writer formatted
 * something. A forward the server completes (no files here) carries its html
 * unwrapped, as the web sends it.
 */
internal fun MailViewModel.outgoing(draft: Draft): Pair<String, String?> {
    val body = MailSignature.append(draft.body, _state.value.signature)
    return body to MarkdownHtml.forSend(body, wrapped = draft.forwardFrom == null || draft.attachments.isNotEmpty() || draft.stored.isNotEmpty())
}

/** The composer closes and the list offers Undo while the server holds the send. */
fun MailViewModel.sendLanded(messageId: String) {
    _state.update { it.copy(sending = false, composing = null, sent = SentNote(messageId)) }
    refresh()
}

/**
 * Stop the held send. 409 means it already left, which the list then says
 * rather than pretending the Undo worked.
 */
fun MailViewModel.undoSend() {
    val note = _state.value.sent ?: return
    if (note.undo != SentNote.Undo.Offered || note.messageId.isEmpty()) return
    _state.update { it.copy(sent = note.copy(undo = SentNote.Undo.Cancelling)) }
    viewModelScope.launch {
        val outcome = when (client.cancelPendingSend(note.messageId)) {
            is MailrsClient.Outcome.Ok -> SentNote.Undo.Cancelled
            is MailrsClient.Outcome.Err -> SentNote.Undo.TooLate
        }
        _state.update { it.copy(sent = note.copy(undo = outcome)) }
        refresh()
    }
}

fun MailViewModel.sentNoticeShown() {
    _state.update { it.copy(sent = null) }
}

/**
 * When a composer opens: read again what the account may send as (rights
 * change while signed in) and its templates, and settle the From line.
 */
fun MailViewModel.loadComposeExtras() {
    viewModelScope.launch {
        val role = client.role()
        if (role is MailrsClient.Outcome.Ok) _state.update { it.copy(role = role.value) }
        val list = when (val r = client.templates()) {
            is MailrsClient.Outcome.Ok -> r.value
            is MailrsClient.Outcome.Err -> emptyList()
        }
        _state.update { s ->
            val draft = s.composing ?: return@update s
            val from = draft.from.ifEmpty { sendAddresses().firstOrNull().orEmpty() }
            s.copy(composing = draft.copy(templates = list, from = from))
        }
    }
}

/** The From choices; one entry means there is nothing to choose. */
fun MailViewModel.sendAddresses(): List<String> {
    val role = _state.value.role
    return SendAddresses.list(role?.address.orEmpty(), role?.sendAs.orEmpty())
}

fun MailViewModel.chooseFrom(address: String) {
    _state.update { s -> s.copy(composing = s.composing?.copy(from = address)) }
}

/** Take the time off a resumed scheduled draft, so Send sends at once. */
fun MailViewModel.unschedule() {
    _state.update { s -> s.copy(composing = s.composing?.copy(scheduledAt = null)) }
}

fun MailViewModel.applyTemplate(template: Template) {
    val draft = _state.value.composing ?: return
    val applied = TemplateApply.apply(template, draft.subject, draft.body, isReply = draft.inReplyTo != null)
    editDraft(subject = applied.subject, body = applied.body)
}
