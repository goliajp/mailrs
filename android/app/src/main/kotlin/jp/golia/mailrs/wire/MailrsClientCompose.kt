package jp.golia.mailrs.wire

import jp.golia.mailrs.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import okhttp3.MultipartBody
import okhttp3.Request

/**
 * How long the server holds an immediate send so Undo can still stop it, and
 * how long the app offers the Undo — the web's numbers (`UNDO_SECONDS` in
 * `web/src/lib/send-mail.ts`, its toast 8 s), the offer ending first so a tap
 * near the end still lands.
 */
object SendHold {
    const val SECONDS = 10
    const val OFFERED_MS = 8_000L
}

/**
 * The addresses a message can leave by: the signed-in one first, then what
 * the server lets this account send as (`send_as` on `GET /api/auth/me`),
 * without repeats — the web's `sendAddresses`.
 */
object SendAddresses {
    fun list(own: String, sendAs: List<String>): List<String> {
        val out = mutableListOf<String>()
        for (raw in listOf(own) + sendAs) {
            val address = raw.trim()
            if (address.isEmpty()) continue
            if (out.any { it.equals(address, ignoreCase = true) }) continue
            out += address
        }
        return out
    }

    /** A reply leaves by the address of mine it was sent to, else the signed-in one. */
    fun replyFrom(options: List<String>, recipients: String): String =
        options.firstOrNull { recipients.contains(it, ignoreCase = true) } ?: options.firstOrNull().orEmpty()
}

/** Backend: `crates/webapi/src/handlers/prefs.rs` — `list_templates` (`TemplateWire`). */
@Serializable
data class Template(
    val id: Long,
    val name: String,
    val subject: String = "",
    @SerialName("text_body") val textBody: String = "",
)

/**
 * `POST /api/mail/send`. Every field is `#[serde(default)]` server-side, but a reply that
 * drops its `in_reply_to` still arrives and starts a new thread — so all are sent explicitly.
 */
@Serializable
data class SendRequest(
    /**
     * Which address this leaves by, or empty for the signed-in one.
     *
     * A reply to mail that arrived at a connected Gmail has to go
     * out through that Gmail: sent from anywhere else it lands in
     * the conversation as a stranger, and half the time the
     * recipient's provider refuses it outright.
     */
    val from: String = "",
    val to: List<String>,
    val cc: List<String> = emptyList(),
    val bcc: List<String> = emptyList(),
    val subject: String,
    val body: String,
    @SerialName("in_reply_to") val inReplyTo: String? = null,
    /**
     * When it should leave, in epoch **seconds**, or null for now.
     *
     * An integer, not a formatted time: the handler reads anything
     * it cannot parse as "not scheduling", which is how the web's
     * ISO 8601 string once made every scheduled send go out at
     * once.
     */
    @SerialName("scheduled_at") val scheduledAt: Long? = null,
    /** The send this is a re-edit of; the server carries its bytes. */
    @SerialName("redraft_of") val redraftOf: String? = null,
    /**
     * Which carried attachments to keep, by index.
     *
     * **Absent and empty mean different things** — absent keeps
     * every carried attachment, an empty list keeps none — so this
     * is nullable rather than defaulted, and a re-edit that removed
     * every file must send `[]` and not nothing.
     */
    @SerialName("redraft_keep") val redraftKeep: List<Int>? = null,
    /**
     * The message being forwarded, by uid.
     *
     * The server re-extracts its attachments and carries them,
     * which is the whole reason a forward does not have to
     * download and re-upload what it is passing on — and the
     * reason a phone can forward a message it has never opened.
     */
    @SerialName("forward_attachments_from") val forwardAttachmentsFrom: Int? = null,
    @SerialName("undo_seconds") val undoSeconds: Int? = null,
    /** The body rendered from its markdown (`MarkdownHtml`), or absent for text alone. */
    @SerialName("html_body") val htmlBody: String? = null,
)

/**
 * What applying a template does, as the web does it: the subject only where
 * the message has none of its own yet (a reply keeps "Re: …"), and the body
 * replaced by the template's text.
 */
object TemplateApply {
    data class Result(val subject: String, val body: String)

    fun apply(template: Template, subject: String, body: String, isReply: Boolean): Result = Result(
        subject = template.subject.takeIf { it.isNotEmpty() && !isReply } ?: subject,
        body = template.textBody.ifEmpty { body },
    )
}

@Serializable
private data class SendAnswer(@SerialName("message_id") val messageId: String = "")

/**
 * `POST /api/mail/send`. The answer is the message id the server gave the
 * send, which is what Undo names (empty if the answer had none).
 */
suspend fun MailrsClient.send(
    to: List<String>,
    subject: String,
    body: String,
    inReplyTo: String?,
    cc: List<String> = emptyList(),
    bcc: List<String> = emptyList(),
    forwardAttachmentsFrom: Int? = null,
    scheduledAt: Long? = null,
    redraftOf: String? = null,
    redraftKeep: List<Int>? = null,
    from: String = "",
    htmlBody: String? = null,
): MailrsClient.Outcome<String> {
    val payload = json.encodeToString(
        SendRequest.serializer(),
        SendRequest(
            from = from,
            to = to,
            cc = cc,
            bcc = bcc,
            subject = subject,
            body = body,
            inReplyTo = inReplyTo,
            forwardAttachmentsFrom = forwardAttachmentsFrom,
            scheduledAt = scheduledAt,
            redraftOf = redraftOf,
            redraftKeep = redraftKeep,
            undoSeconds = SendHold.SECONDS,
            htmlBody = htmlBody,
        ),
    )
    return sentId(post(url("/api/mail/send"), payload, authorized = true))
}

/**
 * `POST /api/mail/send-multipart` — a message with files.
 *
 * Streamed from the content resolver rather than read into a byte array
 * first: a phone photo is a few megabytes and a video is not, and a composer
 * that loaded every attachment into memory to send it would fail on exactly
 * the files worth attaching.
 */
suspend fun MailrsClient.sendMultipart(
    to: List<String>,
    cc: List<String>,
    bcc: List<String>,
    subject: String,
    body: String,
    inReplyTo: String?,
    attachments: List<MailrsClient.Upload>,
    scheduledAt: Long? = null,
    redraftOf: String? = null,
    redraftKeep: List<Int>? = null,
    from: String = "",
    htmlBody: String? = null,
): MailrsClient.Outcome<String> = withContext(Dispatchers.IO) {
    val s = session ?: return@withContext MailrsClient.Outcome.Err(Texts.get(R.string.error_not_signed_in))
    val form = MultipartBody.Builder().setType(MultipartBody.FORM)
    // Repeated fields, one per address: that is what the handler reads
    // (`parts.to.push(...)`), and a comma-joined single field would arrive as
    // one recipient with commas in its name.
    to.forEach { form.addFormDataPart("to", it) }
    cc.forEach { form.addFormDataPart("cc", it) }
    bcc.forEach { form.addFormDataPart("bcc", it) }
    form.addFormDataPart("subject", subject)
    form.addFormDataPart("body", body)
    htmlBody?.let { form.addFormDataPart("html_body", it) }
    // Absent when it is the signed-in address, like every other optional field.
    if (from.isNotEmpty()) form.addFormDataPart("from", from)
    inReplyTo?.let { form.addFormDataPart("in_reply_to", it) }
    // A decimal string of epoch seconds, which is what the handler parses.
    // Anything it cannot parse it treats as "not scheduling" — silently
    // sending at once.
    scheduledAt?.let { form.addFormDataPart("scheduled_at", it.toString()) }
    form.addFormDataPart("undo_seconds", SendHold.SECONDS.toString())
    redraftOf?.let { form.addFormDataPart("redraft_of", it) }
    // **One comma-separated field, not a repeated one**: repeating it cannot
    // express "keep none", because zero occurrences and an empty selection
    // both arrive as no field at all and they mean opposite things.
    redraftKeep?.let { form.addFormDataPart("redraft_keep", it.joinToString(",")) }
    for (a in attachments) {
        form.addFormDataPart("attachments", a.filename, a.body)
    }
    val request = Request.Builder()
        .url("${s.server}/api/mail/send-multipart")
        .header("Authorization", "Bearer ${s.token}")
        .post(form.build())
        .build()
    sentId(send(request))
}

private fun MailrsClient.sentId(r: MailrsClient.Outcome<String>): MailrsClient.Outcome<String> = when (r) {
    is MailrsClient.Outcome.Ok -> MailrsClient.Outcome.Ok(
        runCatching { json.decodeFromString(SendAnswer.serializer(), r.value).messageId }.getOrDefault(""),
    )
    is MailrsClient.Outcome.Err -> r
}

/**
 * `DELETE /api/mail/pending/{message_id}` — stop a send the server is still
 * holding. 409 once it is on its way, which comes back as an `Err`.
 */
suspend fun MailrsClient.cancelPendingSend(messageId: String): MailrsClient.Outcome<String> =
    withContext(Dispatchers.IO) {
        val s = session ?: return@withContext MailrsClient.Outcome.Err(Texts.get(R.string.error_not_signed_in))
        send(
            Request.Builder()
                .url(s.server + "/api/mail/pending/" + enc(messageId))
                .header("Authorization", "Bearer ${s.token}")
                .delete()
                .build(),
        )
    }

/** `GET /api/mail/templates`, a bare array. */
suspend fun MailrsClient.templates(): MailrsClient.Outcome<List<Template>> =
    decode(get("/api/mail/templates"), Template.serializer())
