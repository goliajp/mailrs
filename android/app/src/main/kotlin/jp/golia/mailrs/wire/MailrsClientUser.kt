package jp.golia.mailrs.wire

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.Request
import java.io.IOException

/**
 * The endpoints that belong to a person rather than to the mailbox:
 * drafts, the signature, an unsubscribe, a contact lookup, the bytes of
 * an attachment.
 *
 * Extensions, split out when `MailrsClient` went back over the repo's
 * 500-line limit. Same argument as the admin split: a different job
 * from listing and reading mail, and the plumbing it needs (`get`,
 * `post`, `one`, `decode`) is `internal`, so nothing had to be opened
 * up to move it.
 */
suspend fun MailrsClient.signatures(): MailrsClient.Outcome<List<Wire.Signature>> = decode(
    get("/api/mail/signatures"),
    Wire.Signature.serializer(),
)

/**
 * `GET /api/mail/messages/{uid}/attachments/{index}` — the bytes.
 *
 * **The index is the caller's, not zero.** The server identifies an
 * attachment by its position in the message, and a client that
 * always asked for the first one would show the right name over the
 * wrong file. The stub records what was asked for at
 * `/debug/fetched` precisely so a test can tell those apart.
 */
suspend fun MailrsClient.attachment(uid: Int, index: Int): MailrsClient.Outcome<ByteArray> = withContext(Dispatchers.IO) {
    val s = session ?: return@withContext MailrsClient.Outcome.Err("Not signed in.")
    val request = Request.Builder()
        .url("${s.server}/api/mail/messages/$uid/attachments/$index")
        .header("Authorization", "Bearer ${s.token}")
        .get()
        .build()
    try {
        http.newCall(request).execute().use { response ->
            when {
                response.isSuccessful -> MailrsClient.Outcome.Ok(response.body.bytes())
                response.code == 401 -> {
                    rejected()
                    MailrsClient.Outcome.Err("Signed out — the server rejected this session.")
                }
                else -> MailrsClient.Outcome.Err("The server answered ${response.code}.")
            }
        }
    } catch (e: IOException) {
        MailrsClient.Outcome.Err("Could not reach the server: ${e.message}")
    }
}

/**
 * `POST /api/mail/unsubscribe` — the server leaves the list.
 *
 * Answered with `{ok, status, message}` rather than a status code,
 * because "the sender's endpoint refused" and "we never reached it"
 * are different things to tell a reader.
 */
suspend fun MailrsClient.unsubscribe(threadId: String, uid: Int): MailrsClient.Outcome<Wire.UnsubscribeResult> {
    val body = json.encodeToString(
        Wire.UnsubscribeRequest.serializer(),
        Wire.UnsubscribeRequest(threadId, uid),
    )
    return when (val r = post(url("/api/mail/unsubscribe"), body, authorized = true)) {
        is MailrsClient.Outcome.Ok -> runCatching {
            json.decodeFromString(Wire.UnsubscribeResult.serializer(), r.value)
        }.fold(
            onSuccess = { MailrsClient.Outcome.Ok(it) },
            onFailure = { MailrsClient.Outcome.Err("The server sent a shape this app could not read: ${it.message}") },
        )
        is MailrsClient.Outcome.Err -> r
    }
}

/**
 * `GET /api/contacts?q=` — a bare array of `Name <email>`.
 *
 * Substring on either half, case-insensitively, which is the
 * server's rule and not this app's: matching here as well would
 * mean two answers to one question.
 */
suspend fun MailrsClient.contacts(term: String): MailrsClient.Outcome<List<String>> = decode(
    get("/api/contacts?q=" + enc(term)),
    kotlinx.serialization.serializer<String>(),
)

suspend fun MailrsClient.drafts(): MailrsClient.Outcome<List<Wire.Draft>> = decode(
    get("/api/mail/drafts"),
    Wire.Draft.serializer(),
)

/**
 * `POST /api/mail/drafts` — create or update.
 *
 * The id decides which: present is an in-place update of the same
 * hash field, absent allocates a new one. A composer that dropped
 * the id on the second save would leave a trail of drafts behind
 * one message.
 */
suspend fun MailrsClient.saveDraft(req: Wire.SaveDraftRequest): MailrsClient.Outcome<Long> {
    val payload = json.encodeToString(Wire.SaveDraftRequest.serializer(), req)
    return when (val r = post(url("/api/mail/drafts"), payload, authorized = true)) {
        is MailrsClient.Outcome.Ok -> runCatching {
            json.decodeFromString(Wire.SaveDraftResponse.serializer(), r.value).id
        }.fold(
            onSuccess = { MailrsClient.Outcome.Ok(it) },
            onFailure = { MailrsClient.Outcome.Err("The server sent a shape this app could not read: ${it.message}") },
        )
        is MailrsClient.Outcome.Err -> r
    }
}

suspend fun MailrsClient.deleteDraft(id: Long): MailrsClient.Outcome<String> = withContext(Dispatchers.IO) {
    val s = session ?: return@withContext MailrsClient.Outcome.Err("Not signed in.")
    send(
        Request.Builder()
            .url("${s.server}/api/mail/drafts/$id")
            .header("Authorization", "Bearer ${s.token}")
            .delete()
            .build(),
    )
}
