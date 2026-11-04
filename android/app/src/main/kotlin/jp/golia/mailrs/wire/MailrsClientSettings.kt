package jp.golia.mailrs.wire

import jp.golia.mailrs.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/**
 * Calls for the reading features beside the account's settings, starting
 * with the sectioned Inbox. Every route is the one the web client calls.
 */

/** Any method with an optional JSON body, signed in. */
internal suspend fun MailrsClient.call(
    method: String,
    path: String,
    body: String? = null,
): MailrsClient.Outcome<String> = withContext(Dispatchers.IO) {
    val s = session ?: return@withContext MailrsClient.Outcome.Err(Texts.get(R.string.error_not_signed_in))
    val payload = body?.toRequestBody("application/json".toMediaType())
    send(
        Request.Builder()
            .url(s.server + path)
            .header("Authorization", "Bearer ${s.token}")
            .method(method, payload)
            .build(),
    )
}

@Serializable
private data class AcceptBody(val addresses: List<String>)

@Serializable
data class Accepted(val accepted: Int = 0)

/** `POST /api/senders/accept`: these first contacts leave New, now and for mail to come. */
suspend fun MailrsClient.acceptSenders(addresses: List<String>): MailrsClient.Outcome<Accepted> =
    one(
        call("POST", "/api/senders/accept", json.encodeToString(AcceptBody.serializer(), AcceptBody(addresses))),
        Accepted.serializer(),
    )
