package jp.golia.mailrs

import android.app.Application
import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.ContentUriBody
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.RecipientAutocomplete
import jp.golia.mailrs.wire.ReplyRecipients
import jp.golia.mailrs.wire.ShareIntent
import jp.golia.mailrs.wire.Wire
import kotlinx.coroutines.launch

/**
 * Writing, sending, and everything a message carries.
 *
 * Extensions rather than methods: Kotlin has no partial classes, and
 * `MailViewModel` had reached 1,460 lines against this repo's 500-line
 * limit. Split by what it is about — the composer, its drafts, its
 * attachments and its suggestions — rather than by where the line count
 * happened to fall.
 *
 * The draft lives in `UiState` like everything else. It is not a second
 * copy of the composer's fields; it is the only copy, which is what
 * lets the back gesture save one.
 */
/**
 * Start a message. `replyTo` null is a new one.
 *
 * The reply's recipients, subject and quoted history come from
 * `ReplyRecipients`, which is the web's rules ported once and
 * unit-tested — not re-derived here, where a dropped cc would be
 * invisible until somebody noticed a colleague missing from a
 * thread.
 */
fun MailViewModel.compose(replyTo: Wire.Message? = null, all: Boolean = false) {
    val draft = if (replyTo == null) {
        Draft(id = nextDraftId++)
    } else {
        val me = _state.value.myAddress
        Draft(
            id = nextDraftId++,
            to = if (all) {
                ReplyRecipients.replyAll(replyTo.sender, replyTo.recipients, me).joinToString(", ")
            } else {
                ReplyRecipients.reply(replyTo.sender).joinToString(", ")
            },
            subject = ReplyRecipients.subject(replyTo.subject),
            body = ReplyRecipients.quote(
                replyTo.sender,
                replyTo.internalDate,
                replyTo.textBody.orEmpty(),
            ),
            inReplyTo = replyTo.messageId,
            replyToThreadId = _state.value.open?.threadId,
        )
    }
    _state.value = _state.value.copy(composing = draft, error = null)
}

/**
 * Take on a picked file.
 *
 * The name and size come from the content resolver rather than the
 * URI's last path segment: a document provider's URI is an opaque
 * id, and "msf:1000000042" is not a filename anybody wants to see
 * arrive in their mail.
 */
fun MailViewModel.attach(uris: List<android.net.Uri>) {
    val draft = _state.value.composing ?: return
    val resolver = getApplication<Application>().contentResolver
    val added = uris.mapNotNull { uri ->
        runCatching {
            resolver.query(uri, null, null, null, null)?.use { cursor ->
                val nameAt = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                val sizeAt = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
                if (!cursor.moveToFirst()) return@use null
                Attached(
                    uri = uri,
                    filename = if (nameAt >= 0) cursor.getString(nameAt) else "attachment",
                    size = if (sizeAt >= 0 && !cursor.isNull(sizeAt)) cursor.getLong(sizeAt) else 0,
                )
            }
        }.getOrNull()
    }
    if (added.isEmpty()) return
    _state.value = _state.value.copy(
        composing = draft.copy(attachments = draft.attachments + added),
    )
}

fun MailViewModel.detach(a: Attached) {
    val draft = _state.value.composing ?: return
    _state.value = _state.value.copy(
        composing = draft.copy(attachments = draft.attachments.filterNot { it.uri == a.uri }),
    )
}

/**
 * Start a message another app asked for.
 *
 * `mailto:` from a link, or the share sheet with text and files.
 * Anything that arrives goes into a draft, so leaving still saves
 * it — a shared photo that vanished because the person had second
 * thoughts about the recipient would be worse than no share target
 * at all.
 *
 * Signed out, this does nothing but leave a draft waiting: the
 * sign-in screen shows first, and the composer is there afterwards.
 */
fun MailViewModel.composeFromShare(
    mailto: ShareIntent.Mailto? = null,
    subject: String = "",
    body: String = "",
    attachments: List<android.net.Uri> = emptyList(),
) {
    val draft = Draft(
        id = nextDraftId++,
        to = mailto?.to.orEmpty(),
        cc = mailto?.cc.orEmpty(),
        bcc = mailto?.bcc.orEmpty(),
        subject = mailto?.subject?.takeIf(String::isNotBlank) ?: subject,
        body = mailto?.body?.takeIf(String::isNotBlank) ?: body,
    )
    _state.value = _state.value.copy(composing = draft, error = null)
    if (attachments.isNotEmpty()) attach(attachments)
}

fun MailViewModel.editDraft(
    to: String? = null,
    cc: String? = null,
    bcc: String? = null,
    subject: String? = null,
    body: String? = null,
) {
    val draft = _state.value.composing ?: return
    _state.value = _state.value.copy(
        composing = draft.copy(
            to = to ?: draft.to,
            cc = cc ?: draft.cc,
            bcc = bcc ?: draft.bcc,
            subject = subject ?: draft.subject,
            body = body ?: draft.body,
        ),
    )
}

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
    _state.value = _state.value.copy(composing = null, error = null)
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
            _state.value = _state.value.copy(draftSaved = true)
        }
    }
}

fun MailViewModel.draftNoticeShown() {
    _state.value = _state.value.copy(draftSaved = false)
}

fun MailViewModel.openDrafts() {
    _state.value = _state.value.copy(draftsOpen = true, busy = true, error = null)
    viewModelScope.launch {
        _state.value = when (val r = client.drafts()) {
            is MailrsClient.Outcome.Ok ->
                _state.value.copy(busy = false, drafts = r.value.sortedByDescending { it.updatedAt })
            is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
        }
    }
}

fun MailViewModel.closeDrafts() {
    _state.value = _state.value.copy(draftsOpen = false)
}

/**
 * Reopen a saved draft.
 *
 * Its server id travels with it, so saving again updates the same
 * row rather than leaving a copy behind on every edit.
 */
fun MailViewModel.editSavedDraft(d: Wire.Draft) {
    _state.value = _state.value.copy(
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
    )
}

fun MailViewModel.discardDraft(d: Wire.Draft) {
    _state.value = _state.value.copy(drafts = _state.value.drafts.filterNot { it.id == d.id })
    viewModelScope.launch { client.deleteDraft(d.id) }
}

fun MailViewModel.send() {
    val draft = _state.value.composing ?: return
    val recipients = recipientsIn(draft.to)
    if (recipients.isEmpty()) {
        _state.value = _state.value.copy(error = "A message needs somebody to go to.")
        return
    }
    _state.value = _state.value.copy(sending = true, error = null)
    viewModelScope.launch {
        val resolver = getApplication<Application>().contentResolver
        val r = if (draft.attachments.isEmpty()) {
            client.send(
                recipients,
                draft.subject,
                draft.body,
                draft.inReplyTo,
                cc = recipientsIn(draft.cc),
                bcc = recipientsIn(draft.bcc),
            )
        } else {
            client.sendMultipart(
                to = recipients,
                cc = recipientsIn(draft.cc),
                bcc = recipientsIn(draft.bcc),
                subject = draft.subject,
                body = draft.body,
                inReplyTo = draft.inReplyTo,
                attachments = draft.attachments.map { a ->
                    MailrsClient.Upload(a.filename, ContentUriBody(resolver, a.uri))
                },
            )
        }
        when (r) {
            is MailrsClient.Outcome.Ok -> {
                _state.value = _state.value.copy(sending = false, composing = null, sent = true)
                refresh()
            }
            is MailrsClient.Outcome.Err ->
                // The composer stays open. A send that failed and
                // closed the screen would take the text with it,
                // which is the one thing a person cannot get back.
                _state.value = _state.value.copy(sending = false, error = r.message)
        }
    }
}

fun MailViewModel.acknowledgeSent() {
    _state.value = _state.value.copy(sent = false)
}

/**
 * Fetch an attachment and put it somewhere another app can read.
 *
 * The bytes land in the cache under a per-message directory, so two
 * messages carrying `invoice.pdf` cannot overwrite each other's —
 * the filename is the sender's and is not unique. A `FileProvider`
 * URI is what leaves this app: handing out a `file://` path has
 * been an error since API 24, and a world-readable copy would be a
 * worse way to make it work.
 */
fun MailViewModel.openAttachment(uid: Int, index: Int, att: Wire.Attachment) {
    _state.value = _state.value.copy(openingAttachment = index, error = null)
    viewModelScope.launch {
        _state.value = when (val r = client.attachment(uid, index)) {
            is MailrsClient.Outcome.Ok -> {
                val file = runCatching { writeToCache(uid, index, att.filename, r.value) }
                file.fold(
                    onSuccess = {
                        _state.value.copy(
                            openingAttachment = null,
                            openFile = OpenedFile(it, att.contentType, att.filename),
                        )
                    },
                    onFailure = {
                        _state.value.copy(
                            openingAttachment = null,
                            error = "Could not save ${att.filename}: ${it.message}",
                        )
                    },
                )
            }
            is MailrsClient.Outcome.Err ->
                _state.value.copy(openingAttachment = null, error = r.message)
        }
    }
}

/**
 * Ask the server to leave the list.
 *
 * Only ever the one-click case, and only ever by the message's
 * identity — the advertised URLs identify the subscriber, so the
 * server takes the URL out of the message's own header rather than
 * being told where to post.
 *
 * A refusal is kept per message and shown, because an unsubscribe
 * that failed and looks like one that worked is how people end up
 * tapping it every week for a year.
 */
fun MailViewModel.unsubscribe(threadId: String, uid: Int) {
    _state.value = _state.value.copy(
        unsubscribing = _state.value.unsubscribing + (uid to Unsubscribing.Working),
    )
    viewModelScope.launch {
        val outcome = client.unsubscribe(threadId, uid)
        val verdict = when {
            outcome is MailrsClient.Outcome.Ok && outcome.value.ok -> Unsubscribing.Done
            else -> Unsubscribing.Failed
        }
        _state.value = _state.value.copy(
            unsubscribing = _state.value.unsubscribing + (uid to verdict),
        )
    }
}

fun MailViewModel.attachmentOpened() {
    _state.value = _state.value.copy(openFile = null)
}

private fun MailViewModel.writeToCache(uid: Int, index: Int, filename: String, bytes: ByteArray): java.io.File {
    val dir = java.io.File(getApplication<Application>().cacheDir, "attachments/$uid-$index")
    dir.mkdirs()
    // The sender chose this name. A name that walks out of the
    // directory — "../../databases/x" — must land in the directory
    // anyway, so only the last path component is kept.
    val safe = filename.substringAfterLast('/').ifBlank { "attachment" }
    val file = java.io.File(dir, safe)
    file.writeBytes(bytes)
    return file
}

/**
 * Contacts for the name being typed.
 *
 * Asked per field, so a suggestion for Cc cannot land in To. The
 * token rule is `RecipientAutocomplete`'s and the matching is the
 * server's — matching again here would be a second answer to one
 * question.
 */
fun MailViewModel.suggestContacts(field: RecipientField, line: String) {
    val token = RecipientAutocomplete.currentToken(line)
    if (!RecipientAutocomplete.shouldSuggest(token)) {
        _state.value = _state.value.copy(suggestions = emptyList(), suggestingFor = null)
        return
    }
    contactToken++
    val mine = contactToken
    viewModelScope.launch {
        val r = client.contacts(token)
        if (mine != contactToken) return@launch
        _state.value = when (r) {
            is MailrsClient.Outcome.Ok ->
                _state.value.copy(suggestions = r.value, suggestingFor = field)
            // A suggestion list that cannot be fetched is not an
            // error worth a banner: the person can type the address.
            is MailrsClient.Outcome.Err ->
                _state.value.copy(suggestions = emptyList(), suggestingFor = null)
        }
    }
}

fun MailViewModel.clearSuggestions() {
    if (_state.value.suggestions.isEmpty()) return
    _state.value = _state.value.copy(suggestions = emptyList(), suggestingFor = null)
}
