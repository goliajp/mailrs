package jp.golia.mailrs.wire

import java.io.IOException
import jp.golia.mailrs.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MultipartBody
import okhttp3.Request

/**
 * `POST /api/mail/draft-attachments` — one file, streamed from the phone,
 * answered with the `DraftAttachment` a draft save names.
 *
 * A refusal carries the server's own reason (413 says the limit, 507 that
 * the account's uploads are full), so that is what comes back to show;
 * the app sets no size limit of its own.
 */
suspend fun MailrsClient.uploadDraftFile(upload: MailrsClient.Upload): MailrsClient.Outcome<DraftAttachment> =
    withContext(Dispatchers.IO) {
        val s = session ?: return@withContext MailrsClient.Outcome.Err(Texts.get(R.string.error_not_signed_in))
        val form = MultipartBody.Builder().setType(MultipartBody.FORM)
            .addFormDataPart("file", upload.filename, upload.body)
            .build()
        val request = Request.Builder()
            .url("${s.server}/api/mail/draft-attachments")
            .header("Authorization", "Bearer ${s.token}")
            .post(form)
            .build()
        when (val r = one(send(request), DraftAttachment.serializer())) {
            is MailrsClient.Outcome.Ok -> r
            is MailrsClient.Outcome.Err -> MailrsClient.Outcome.Err(r.code ?: r.message)
        }
    }

/**
 * `GET /api/mail/draft-attachments/{blob_id}` — the bytes of a file this
 * account keeps with a draft.
 */
suspend fun MailrsClient.draftFile(a: DraftAttachment): MailrsClient.Outcome<ByteArray> = withContext(Dispatchers.IO) {
    val s = session ?: return@withContext MailrsClient.Outcome.Err(Texts.get(R.string.error_not_signed_in))
    val request = Request.Builder()
        .url(s.server + "/api/mail/draft-attachments/" + enc(a.blobId) + "?name=" + enc(a.name) + "&type=" + enc(a.contentType))
        .header("Authorization", "Bearer ${s.token}")
        .get()
        .build()
    try {
        http.newCall(request).execute().use { response ->
            when {
                response.isSuccessful -> MailrsClient.Outcome.Ok(response.body.bytes())
                response.code == 401 -> {
                    rejected()
                    MailrsClient.Outcome.Err(Texts.get(R.string.error_signed_out))
                }
                else -> MailrsClient.Outcome.Err(Texts.get(R.string.error_status, response.code))
            }
        }
    } catch (e: IOException) {
        MailrsClient.Outcome.Err(Texts.get(R.string.error_unreachable, e.message))
    }
}
