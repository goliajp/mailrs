package jp.golia.mailrs

import android.app.Application
import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.accounts.SafeFilename
import jp.golia.mailrs.wire.ContentUriBody
import jp.golia.mailrs.wire.DraftAttachment
import jp.golia.mailrs.wire.DraftSend
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.SaveDraftRequest
import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.wire.draftFile
import jp.golia.mailrs.wire.saveDraft
import jp.golia.mailrs.wire.uploadDraftFile
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Files in the composer, kept with the draft.
 *
 * A picked file is uploaded the moment it is added; once the server has
 * it the draft names it (`stored`) on every save, so leaving the composer
 * does not lose it. One the server refused stays on the phone with the
 * server's reason and goes in the send's form.
 */

/**
 * Take on picked files and start uploading each.
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
                1 -> Texts.get(R.string.compose_one_lost)
                else -> Texts.get(R.string.compose_many_lost, lost)
            },
        )
    }
    for (a in added) upload(draft.id, a)
}

fun MailViewModel.detach(a: Attached) {
    val draft = _state.value.composing ?: return
    _state.update { it.copy(
        composing = draft.copy(attachments = draft.attachments.filterNot { it.uri == a.uri }),
    ) }
}

/** Drop a file the server keeps; the next save leaves it off the draft. */
fun MailViewModel.detachStored(index: Int) {
    _state.update { s ->
        val draft = s.composing ?: return@update s
        s.copy(composing = draft.copy(stored = draft.stored.filterIndexed { i, _ -> i != index }))
    }
}

/**
 * Upload one picked file for the draft `draftId`. The answer lands only
 * on that draft, and only while the file is still on it: a file removed
 * while it uploaded stays removed.
 */
private fun MailViewModel.upload(draftId: Int, a: Attached) {
    val resolver = getApplication<Application>().contentResolver
    viewModelScope.launch {
        val r = client.uploadDraftFile(MailrsClient.Upload(a.filename, ContentUriBody(resolver, a.uri)))
        _state.update { s ->
            val draft = s.composing
            if (draft == null || draft.id != draftId || draft.attachments.none { it.uri == a.uri }) return@update s
            val rest = draft.attachments.filterNot { it.uri == a.uri }
            when (r) {
                is MailrsClient.Outcome.Ok ->
                    s.copy(composing = draft.copy(attachments = rest, stored = draft.stored + r.value))
                is MailrsClient.Outcome.Err -> s.copy(composing = draft.copy(
                    attachments = draft.attachments.map { if (it.uri == a.uri) it.copy(refusal = r.message) else it },
                ))
            }
        }
    }
}

/**
 * Fetch a file the server keeps with the draft and hand it to another
 * app, through the same cache and `FileProvider` as a message's files.
 */
fun MailViewModel.openDraftFile(a: DraftAttachment) {
    viewModelScope.launch {
        when (val r = client.draftFile(a)) {
            is MailrsClient.Outcome.Ok -> {
                // the blob id is the content's hash, so one directory per file
                val dir = java.io.File(getApplication<Application>().cacheDir, "attachments/draft-" + a.blobId.take(17))
                dir.mkdirs()
                val file = java.io.File(dir, SafeFilename.of(a.name))
                file.writeBytes(r.value)
                _state.update { it.copy(openFile = OpenedFile(file, a.contentType, a.name)) }
            }
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}

/** What a save of this draft sends: always the files it shows. */
fun Draft.saveRequest(): SaveDraftRequest = SaveDraftRequest(
    id = serverId,
    to = to,
    cc = cc,
    bcc = bcc,
    subject = subject,
    body = body,
    replyToThreadId = replyToThreadId,
    from = from,
    scheduledAt = scheduledAt,
    attachments = stored,
)

/**
 * Save the draft right before its send, so the files the server attaches
 * are the ones shown. The id a first save allocates is kept, so a send
 * that then fails does not leave a second draft on the next try.
 */
internal suspend fun MailViewModel.saveBeforeSend(draft: Draft): DraftSend.Next {
    if (!DraftSend.savesFirst(draft.stored.size, draft.serverId)) return DraftSend.Next.Send(null)
    val saved = (client.saveDraft(draft.saveRequest()) as? MailrsClient.Outcome.Ok)?.value
    if (saved != null) {
        _state.update { s ->
            val current = s.composing
            if (current == null || current.id != draft.id) return@update s
            s.copy(composing = current.copy(serverId = saved))
        }
    }
    return DraftSend.afterSave(draft.stored.size, saved)
}
