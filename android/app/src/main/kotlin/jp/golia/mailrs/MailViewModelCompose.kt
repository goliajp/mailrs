package jp.golia.mailrs

import jp.golia.mailrs.wire.cancelScheduled
import jp.golia.mailrs.wire.scheduledSends
import jp.golia.mailrs.wire.SendSchedule
import jp.golia.mailrs.wire.sends
import jp.golia.mailrs.wire.sentMessages
import jp.golia.mailrs.wire.SendJoin
import kotlinx.coroutines.flow.update
import android.app.Application
import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.ContentUriBody
import jp.golia.mailrs.wire.attachment
import jp.golia.mailrs.wire.contacts
import jp.golia.mailrs.wire.deleteDraft
import jp.golia.mailrs.wire.drafts
import jp.golia.mailrs.wire.saveDraft
import jp.golia.mailrs.wire.unsubscribe
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.MailSignature
import jp.golia.mailrs.wire.RecentRecipients
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
fun MailViewModel.compose(
    replyTo: Wire.Message? = null,
    all: Boolean = false,
    forward: Boolean = false,
) {
    val draft = when {
        replyTo == null -> Draft(id = nextDraftId++)

        // **A forward starts empty and keeps everything else.** It goes
        // to somebody new, so the recipients are theirs to type; the
        // subject and the quoted original are the message being passed
        // on, and `forwardFrom` is what lets the server carry that
        // message's attachments without this phone downloading them.
        //
        // It is not a reply: `in_reply_to` would thread the forward
        // into the original conversation, where the person receiving it
        // has never been.
        forward -> Draft(
            id = nextDraftId++,
            subject = ReplyRecipients.subject(replyTo.subject, forwarding = true),
            body = ReplyRecipients.quote(
                replyTo.sender,
                replyTo.internalDate,
                replyTo.textBody.orEmpty(),
            ),
            forwardFrom = replyTo.uid,
        )

        else -> Draft(
            id = nextDraftId++,
            to = if (all) {
                ReplyRecipients.replyAll(replyTo.sender, replyTo.recipients, _state.value.myAddress)
                    .joinToString(", ")
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
    _state.update { it.copy(composing = draft, error = null) }
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
    // **Say when one did not come through.** A provider can refuse to
    // answer for a URI — a file on a share that has gone away, a
    // document the granting app has since revoked — and picking three
    // files to find two attached, with nothing said, is the reader
    // being quietly lied to about what is going out.
    val lost = uris.size - added.size
    _state.update { state ->
        state.copy(
            composing = if (added.isEmpty()) {
                state.composing
            } else {
                draft.copy(attachments = draft.attachments + added)
            },
            error = when (lost) {
                0 -> state.error
                1 -> "One file could not be read and was not attached."
                else -> "$lost files could not be read and were not attached."
            },
        )
    }
}

fun MailViewModel.detach(a: Attached) {
    val draft = _state.value.composing ?: return
    _state.update { it.copy(
        composing = draft.copy(attachments = draft.attachments.filterNot { it.uri == a.uri }),
    ) }
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
    _state.update { it.copy(composing = draft, error = null) }
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
    _state.update { it.copy(
        composing = draft.copy(
            to = to ?: draft.to,
            cc = cc ?: draft.cc,
            bcc = bcc ?: draft.bcc,
            subject = subject ?: draft.subject,
            body = body ?: draft.body,
        ),
    ) }
}
/**
 * @param schedule when it should leave. [SendSchedule.Now] sends at
 *   once; anything else hands the server a `scheduled_at` and the
 *   sender's sweep promotes it when it is due.
 */
fun MailViewModel.send(schedule: SendSchedule = SendSchedule.Now) {
    val draft = _state.value.composing ?: return
    val recipients = recipientsIn(draft.to)
    if (recipients.isEmpty()) {
        _state.update { it.copy(error = "A message needs somebody to go to.") }
        return
    }
    _state.update { it.copy(sending = true, error = null) }
    viewModelScope.launch {
        val resolver = getApplication<Application>().contentResolver
        val r = if (draft.attachments.isEmpty()) {
            client.send(
                recipients,
                draft.subject,
                // Signed on the way out, not in the composer: what
                // was typed is what is shown, and a reply that already
                // carries a separator is left alone.
                MailSignature.append(draft.body, _state.value.signature),
                draft.inReplyTo,
                cc = recipientsIn(draft.cc),
                bcc = recipientsIn(draft.bcc),
                forwardAttachmentsFrom = draft.forwardFrom,
                scheduledAt = schedule.fireDate(java.time.ZonedDateTime.now()),
                redraftOf = draft.redraftOf,
                redraftKeep = draft.keptCarried(),
            )
        } else {
            client.sendMultipart(
                to = recipients,
                cc = recipientsIn(draft.cc),
                bcc = recipientsIn(draft.bcc),
                subject = draft.subject,
                body = MailSignature.append(draft.body, _state.value.signature),
                inReplyTo = draft.inReplyTo,
                attachments = draft.attachments.map { a ->
                    MailrsClient.Upload(a.filename, ContentUriBody(resolver, a.uri))
                },
                scheduledAt = schedule.fireDate(java.time.ZonedDateTime.now()),
                redraftOf = draft.redraftOf,
                redraftKeep = draft.keptCarried(),
            )
        }
        when (r) {
            is MailrsClient.Outcome.Ok -> {
                // The share sheet's top row is built from people
                // actually written to. Recorded here rather than from
                // the address book, because a sheet offering everyone
                // this account has *received* from would put a mailing
                // list one tap from a photo.
                RecentRecipients.remember(getApplication(), recipients)
                _state.update { it.copy(sending = false, composing = null, sent = true) }
                refresh()
            }
            is MailrsClient.Outcome.Err ->
                // The composer stays open. A send that failed and
                // closed the screen would take the text with it,
                // which is the one thing a person cannot get back.
                _state.update { it.copy(sending = false, error = r.message) }
        }
    }
}

fun MailViewModel.acknowledgeSent() {
    _state.update { it.copy(sent = false) }
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
    _state.update { it.copy(openingAttachment = index, error = null) }
    viewModelScope.launch {
        // The parameter is named because `fold` binds `it` too, and a
        // bare `it.copy(...)` in there means the file rather than the
        // state — which compiles in some shapes and is never what was
        // meant.
        _state.update { state ->
            when (val r = client.attachment(uid, index)) {
                is MailrsClient.Outcome.Ok ->
                    runCatching { writeToCache(uid, index, att.filename, r.value) }.fold(
                        onSuccess = { file ->
                            state.copy(
                                openingAttachment = null,
                                openFile = OpenedFile(file, att.contentType, att.filename),
                            )
                        },
                        onFailure = { failure ->
                            state.copy(
                                openingAttachment = null,
                                error = "Could not save ${att.filename}: ${failure.message}",
                            )
                        },
                    )

                is MailrsClient.Outcome.Err ->
                    state.copy(openingAttachment = null, error = r.message)
            }
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
    _state.update { it.copy(
        unsubscribing = _state.value.unsubscribing + (uid to Unsubscribing.Working),
    ) }
    viewModelScope.launch {
        val outcome = client.unsubscribe(threadId, uid)
        val verdict = when {
            outcome is MailrsClient.Outcome.Ok && outcome.value.ok -> Unsubscribing.Done
            else -> Unsubscribing.Failed
        }
        _state.update { it.copy(
            unsubscribing = _state.value.unsubscribing + (uid to verdict),
        ) }
    }
}

fun MailViewModel.attachmentOpened() {
    _state.update { it.copy(openFile = null) }
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
        _state.update { it.copy(suggestions = emptyList(), suggestingFor = null) }
        return
    }
    contactToken++
    val mine = contactToken
    viewModelScope.launch {
        val r = client.contacts(token)
        if (mine != contactToken) return@launch
        _state.update {
            when (r) {
            is MailrsClient.Outcome.Ok ->
                it.copy(suggestions = r.value, suggestingFor = field)
            // A suggestion list that cannot be fetched is not an
            // error worth a banner: the person can type the address.
            is MailrsClient.Outcome.Err ->
                it.copy(suggestions = emptyList(), suggestingFor = null)
        }
        }
    }
}

fun MailViewModel.clearSuggestions() {
    if (_state.value.suggestions.isEmpty()) return
    _state.update { it.copy(suggestions = emptyList(), suggestingFor = null) }
}

/**
 * Which carried files survive an edit, or null when nothing was
 * carried.
 *
 * Null and empty are not the same on the wire: absent keeps every
 * carried attachment and `[]` keeps none. A draft that never carried
 * anything must send absent, or the server would read "keep none" as
 * an instruction about files it is not holding.
 */
fun Draft.keptCarried(): List<Int>? {
    if (redraftOf == null || carried.isEmpty()) return null
    return carried.map { it.index }.filterNot { it in carriedDropped }
}

/** Drop one of the files the server is holding for this re-edit. */
fun MailViewModel.dropCarried(index: Int) {
    _state.update { s ->
        val draft = s.composing ?: return@update s
        s.copy(composing = draft.copy(carriedDropped = draft.carriedDropped + index))
    }
}
