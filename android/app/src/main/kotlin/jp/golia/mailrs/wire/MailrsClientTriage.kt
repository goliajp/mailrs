package jp.golia.mailrs.wire

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.MediaType.Companion.toMediaType

/**
 * Filing mail with the routes the web uses: the batch route for the
 * verbs it carries, each conversation's own route for the rest.
 */

/**
 * Do [action] to [threadIds].
 *
 * One request for a batch verb; one per conversation otherwise, stopping
 * at the first refusal so the caller can put the rows back — a partial
 * success reported as success leaves rows off the screen that are still
 * in the mailbox.
 *
 * [until] is when a snooze ends, in epoch seconds — the reader's choice
 * from [SnoozeChoice], so it is required for [RowAction.Snooze].
 */
suspend fun MailrsClient.act(
    action: RowAction,
    threadIds: List<String>,
    labelId: String? = null,
    until: Long? = null,
): MailrsClient.Outcome<Unit> {
    action.batch?.let { verb -> return batchVerb(verb, threadIds, labelId) }
    for (id in threadIds) {
        val r = actOne(action, id, until)
        if (r is MailrsClient.Outcome.Err) return r
    }
    return MailrsClient.Outcome.Ok(Unit)
}

private suspend fun MailrsClient.actOne(
    action: RowAction,
    threadId: String,
    until: Long?,
): MailrsClient.Outcome<String> {
    val path = "/api/conversations/${enc(threadId)}"
    return when (action) {
        RowAction.DeleteForever -> request("DELETE", "$path/forever", null)
        RowAction.MarkJunk -> post(url("$path/mark-junk"), "{}", authorized = true)
        RowAction.MarkNotJunk -> post(url("$path/mark-not-junk"), "{}", authorized = true)
        RowAction.MarkNotification -> post(url("$path/mark-notification"), "{}", authorized = true)
        RowAction.MarkPromotion -> post(url("$path/mark-promotion"), "{}", authorized = true)
        RowAction.MoveToInbox -> post(url("$path/move-to-inbox"), "{}", authorized = true)
        RowAction.Pin -> post(url("$path/pin"), "{}", authorized = true)
        RowAction.Unpin -> post(url("$path/unpin"), "{}", authorized = true)
        RowAction.Snooze -> {
            val body = json.encodeToString(
                WireTriage.SnoozeBody.serializer(),
                WireTriage.SnoozeBody(checkNotNull(until)),
            )
            request("PUT", "$path/snooze", body)
        }
        RowAction.Unsnooze -> request("DELETE", "$path/snooze", null)
        RowAction.Release -> post(url("/api/quarantine/${enc(threadId)}/release"), "{}", authorized = true)
        // the batch verbs never reach here
        else -> MailrsClient.Outcome.Err(Texts.get(jp.golia.mailrs.R.string.error_action_unknown))
    }
}

private suspend fun MailrsClient.batchVerb(
    verb: String,
    threadIds: List<String>,
    labelId: String?,
): MailrsClient.Outcome<Unit> {
    val payload = json.encodeToString(
        Wire.BatchRequest.serializer(),
        Wire.BatchRequest(verb, threadIds, labelId),
    )
    return when (val r = post(url("/api/conversations/batch"), payload, authorized = true)) {
        is MailrsClient.Outcome.Ok -> runCatching {
            json.decodeFromString(Wire.BatchResult.serializer(), r.value)
        }.fold(
            onSuccess = { result ->
                when (result.success && result.failed == 0) {
                    true -> MailrsClient.Outcome.Ok(Unit)
                    false -> MailrsClient.Outcome.Err(
                        result.message ?: Texts.get(
                            jp.golia.mailrs.R.string.error_batch_refused,
                            result.failed,
                            threadIds.size,
                        ),
                    )
                }
            },
            onFailure = {
                MailrsClient.Outcome.Err(
                    Texts.get(jp.golia.mailrs.R.string.error_shape, it.message.orEmpty()),
                )
            },
        )
        is MailrsClient.Outcome.Err -> r
    }
}

/** `GET /api/labels` — the reader's labels, by name. */
suspend fun MailrsClient.labels(): MailrsClient.Outcome<List<WireTriage.Label>> =
    decode(get("/api/labels"), WireTriage.Label.serializer()).map { rows -> rows.sortedBy { it.name.lowercase() } }

/**
 * `GET /api/quarantine` — Review, a page of it.
 *
 * Its own endpoint and its own answer shape (`{items, total}`), which is
 * why [MailrsClient.conversations] sends a held list here. Paged the way
 * the list is ([ThreadPage]).
 */
suspend fun MailrsClient.heldList(page: ThreadPage.Next?): MailrsClient.Outcome<List<Wire.Conversation>> =
    one(
        get("/api/quarantine?limit=50" + ThreadPage.query(page)),
        WireTriage.HeldList.serializer(),
    ).map { it.items }

private suspend fun MailrsClient.request(
    method: String,
    path: String,
    body: String?,
): MailrsClient.Outcome<String> = withContext(Dispatchers.IO) {
    val s = session ?: return@withContext MailrsClient.Outcome.Err(
        Texts.get(jp.golia.mailrs.R.string.error_not_signed_in),
    )
    val payload = body?.toRequestBody("application/json".toMediaType())
    send(
        Request.Builder()
            .url(s.server + path)
            .header("Authorization", "Bearer ${s.token}")
            .method(method, payload)
            .build(),
    )
}
