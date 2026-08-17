package jp.golia.mailrs.wire

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.MultipartBody
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * The mailrs REST API, as this app uses it.
 *
 * One bearer token, sent as `Authorization: Bearer` — the same token the
 * web client keeps in `localStorage` under `mailrs_auth` and the iOS app
 * keeps in the keychain.
 *
 * Errors are a sealed result rather than exceptions-as-control-flow,
 * because the difference between "your password is wrong", "the network
 * is down" and "the server said something we could not parse" is the
 * difference between three different things to tell the user, and an
 * exception type flattens them into "something failed".
 */
class MailrsClient(private val store: TokenStore) {

    private val http = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

    var session: TokenStore.Session? = store.read()
        private set

    /**
     * Where a test points the app.
     *
     * The iOS suite passes `-mailrsBaseURL http://localhost:6039` as a
     * launch argument and runs against `ios/Testing/stub-api.py`; the
     * instrumented suite here passes the same URL as an instrumentation
     * argument, so **both clients drive the same stub** rather than each
     * having its own idea of what the server sends. A test that needs
     * somebody's real password is a test nobody runs.
     *
     * Null in a shipped build: `InstrumentationRegistry` is not on the
     * classpath there, so this is only ever set by the test runner.
     */
    var baseUrlOverride: String? = null

    fun signOut() {
        session = null
        store.clear()
    }

    suspend fun login(server: String, username: String, password: String): Outcome<Unit> {
        val base = baseUrlOverride ?: normalise(server)
        val body = json.encodeToString(Wire.LoginRequest.serializer(), Wire.LoginRequest(username, password))
        return when (val r = post("$base/api/auth/login", body, authorized = false)) {
            is Outcome.Ok -> runCatching {
                json.decodeFromString(Wire.LoginResponse.serializer(), r.value)
            }.fold(
                onSuccess = {
                    val s = TokenStore.Session(base, it.token)
                    store.write(s)
                    session = s
                    Outcome.Ok(Unit)
                },
                onFailure = { Outcome.Err("The server's answer to login was not what this app expects.") },
            )
            is Outcome.Err -> r
        }
    }

    suspend fun conversations(list: MailList): Outcome<List<Wire.Conversation>> = decode(
        get("/api/conversations?limit=50&" + list.axes.query()),
        Wire.Conversation.serializer(),
    )

    /**
     * `GET /api/conversations/search?q=`.
     *
     * **The order that comes back is the order to show.** The server
     * walks ranked hit ids and hydrates them in that order, so a client
     * that helpfully re-sorts by date throws the ranking away and shows
     * the least relevant match first. The array is used as it arrives.
     */
    suspend fun search(term: String, list: MailList): Outcome<List<Wire.Conversation>> = decode(
        // Scoped to the same list. The axes travel together for exactly
        // this reason: a search that dropped them would answer a
        // question about Inbox while the heading said Junk.
        get("/api/conversations/search?q=" + enc(term) + "&" + list.axes.query()),
        Wire.Conversation.serializer(),
    )

    suspend fun thread(threadId: String): Outcome<List<Wire.Message>> = decode(
        get("/api/conversations/${enc(threadId)}"),
        Wire.Message.serializer(),
    )

    suspend fun send(
        to: List<String>,
        subject: String,
        body: String,
        inReplyTo: String?,
        cc: List<String> = emptyList(),
        bcc: List<String> = emptyList(),
    ): Outcome<Unit> {
        val payload = json.encodeToString(
            Wire.SendRequest.serializer(),
            Wire.SendRequest(
                to = to,
                cc = cc,
                bcc = bcc,
                subject = subject,
                body = body,
                inReplyTo = inReplyTo,
            ),
        )
        return when (val r = post(url("/api/mail/send"), payload, authorized = true)) {
            is Outcome.Ok -> Outcome.Ok(Unit)
            is Outcome.Err -> r
        }
    }

    /**
     * One of the verbs `conversation_verbs.rs` accepts: `archive`,
     * `unarchive`, `read`, `unread`, `delete`.
     *
     * Named rather than free-form so a typo is a compile error here and
     * not a 400 nobody sees — the row has already moved optimistically
     * by the time this runs.
     */
    suspend fun batch(action: Verb, threadIds: List<String>): Outcome<Unit> {
        val payload = json.encodeToString(
            Wire.BatchRequest.serializer(),
            Wire.BatchRequest(action.wire, threadIds),
        )
        return when (val r = post(url("/api/conversations/batch"), payload, authorized = true)) {
            is Outcome.Ok -> Outcome.Ok(Unit)
            is Outcome.Err -> r
        }
    }

    /**
     * The batch verbs the server accepts.
     *
     * Exactly the set in `conversation_verbs.rs` — an unknown one is a
     * 500 with "unknown batch action" and no other signal, so the names
     * are the wire's rather than this app's.
     */
    enum class Verb(val wire: String) {
        Archive("archive"),
        Unarchive("unarchive"),
        Read("read"),
        Unread("unread"),
        Star("star"),
        Unstar("unstar"),
        Delete("delete"),
    }

    /** Mark a thread read. Best-effort: the list still works if it fails. */
    suspend fun markRead(threadId: String) {
        post(url("/api/conversations/${enc(threadId)}/read"), "{}", authorized = true)
    }

    /**
     * `GET /api/mail/messages/{uid}/attachments/{index}` — the bytes.
     *
     * **The index is the caller's, not zero.** The server identifies an
     * attachment by its position in the message, and a client that
     * always asked for the first one would show the right name over the
     * wrong file. The stub records what was asked for at
     * `/debug/fetched` precisely so a test can tell those apart.
     */
    suspend fun attachment(uid: Int, index: Int): Outcome<ByteArray> = withContext(Dispatchers.IO) {
        val s = session ?: return@withContext Outcome.Err("Not signed in.")
        val request = Request.Builder()
            .url("${s.server}/api/mail/messages/$uid/attachments/$index")
            .header("Authorization", "Bearer ${s.token}")
            .get()
            .build()
        try {
            http.newCall(request).execute().use { response ->
                when {
                    response.isSuccessful -> Outcome.Ok(response.body.bytes())
                    response.code == 401 -> Outcome.Err("Signed out — the server rejected this session.")
                    else -> Outcome.Err("The server answered ${response.code}.")
                }
            }
        } catch (e: IOException) {
            Outcome.Err("Could not reach the server: ${e.message}")
        }
    }

    /**
     * `POST /api/mail/unsubscribe` — the server leaves the list.
     *
     * Answered with `{ok, status, message}` rather than a status code,
     * because "the sender's endpoint refused" and "we never reached it"
     * are different things to tell a reader.
     */
    suspend fun unsubscribe(threadId: String, uid: Int): Outcome<Wire.UnsubscribeResult> {
        val body = json.encodeToString(
            Wire.UnsubscribeRequest.serializer(),
            Wire.UnsubscribeRequest(threadId, uid),
        )
        return when (val r = post(url("/api/mail/unsubscribe"), body, authorized = true)) {
            is Outcome.Ok -> runCatching {
                json.decodeFromString(Wire.UnsubscribeResult.serializer(), r.value)
            }.fold(
                onSuccess = { Outcome.Ok(it) },
                onFailure = { Outcome.Err("The server sent a shape this app could not read: ${it.message}") },
            )
            is Outcome.Err -> r
        }
    }

    /**
     * `GET /api/contacts?q=` — a bare array of `Name <email>`.
     *
     * Substring on either half, case-insensitively, which is the
     * server's rule and not this app's: matching here as well would
     * mean two answers to one question.
     */
    suspend fun contacts(term: String): Outcome<List<String>> = decode(
        get("/api/contacts?q=" + enc(term)),
        kotlinx.serialization.serializer<String>(),
    )

    suspend fun drafts(): Outcome<List<Wire.Draft>> = decode(
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
    suspend fun saveDraft(req: Wire.SaveDraftRequest): Outcome<Long> {
        val payload = json.encodeToString(Wire.SaveDraftRequest.serializer(), req)
        return when (val r = post(url("/api/mail/drafts"), payload, authorized = true)) {
            is Outcome.Ok -> runCatching {
                json.decodeFromString(Wire.SaveDraftResponse.serializer(), r.value).id
            }.fold(
                onSuccess = { Outcome.Ok(it) },
                onFailure = { Outcome.Err("The server sent a shape this app could not read: ${it.message}") },
            )
            is Outcome.Err -> r
        }
    }

    suspend fun deleteDraft(id: Long): Outcome<String> = withContext(Dispatchers.IO) {
        val s = session ?: return@withContext Outcome.Err("Not signed in.")
        send(
            Request.Builder()
                .url("${s.server}/api/mail/drafts/$id")
                .header("Authorization", "Bearer ${s.token}")
                .delete()
                .build(),
        )
    }

    /**
     * `POST /api/mail/send-multipart` — a message with files.
     *
     * Streamed from the content resolver rather than read into a byte
     * array first: a phone photo is a few megabytes and a video is not,
     * and a composer that loaded every attachment into memory to send it
     * would fail on exactly the files worth attaching.
     */
    suspend fun sendMultipart(
        to: List<String>,
        cc: List<String>,
        bcc: List<String>,
        subject: String,
        body: String,
        inReplyTo: String?,
        attachments: List<Upload>,
    ): Outcome<Unit> = withContext(Dispatchers.IO) {
        val s = session ?: return@withContext Outcome.Err("Not signed in.")
        val form = MultipartBody.Builder().setType(MultipartBody.FORM)
        // Repeated fields, one per address: that is what the handler
        // reads (`parts.to.push(...)`), and a comma-joined single field
        // would arrive as one recipient with commas in its name.
        to.forEach { form.addFormDataPart("to", it) }
        cc.forEach { form.addFormDataPart("cc", it) }
        bcc.forEach { form.addFormDataPart("bcc", it) }
        form.addFormDataPart("subject", subject)
        form.addFormDataPart("body", body)
        inReplyTo?.let { form.addFormDataPart("in_reply_to", it) }
        for (a in attachments) {
            form.addFormDataPart("attachments", a.filename, a.body)
        }
        val request = Request.Builder()
            .url("${s.server}/api/mail/send-multipart")
            .header("Authorization", "Bearer ${s.token}")
            .post(form.build())
            .build()
        when (val r = send(request)) {
            is Outcome.Ok -> Outcome.Ok(Unit)
            is Outcome.Err -> r
        }
    }

    /** One file on its way out: a name, a type, and a body to stream. */
    data class Upload(val filename: String, val body: RequestBody)

    // ── Operator ────────────────────────────────────────────────────

    suspend fun accounts(): Outcome<List<Admin.Account>> =
        one(get("/api/admin/accounts"), Admin.AccountList.serializer()).map { it.items }

    suspend fun aliases(): Outcome<List<Admin.Alias>> =
        one(get("/api/admin/aliases"), Admin.AliasList.serializer()).map { it.items }

    suspend fun domains(): Outcome<List<Admin.Domain>> =
        one(get("/api/admin/domains"), Admin.DomainList.serializer()).map { it.items }

    suspend fun queue(): Outcome<List<Admin.QueueJob>> =
        one(get("/api/admin/queues"), Admin.QueueList.serializer()).map { it.items }

    suspend fun dmarcReports(): Outcome<List<Admin.DmarcReport>> =
        one(get("/api/admin/dmarc/reports"), Admin.DmarcList.serializer()).map { it.items }

    suspend fun auditLog(): Outcome<List<Admin.AuditEntry>> =
        one(get("/api/admin/audit-log"), Admin.AuditList.serializer()).map { it.items }

    suspend fun agentKeys(): Outcome<List<Admin.AgentKey>> =
        one(get("/api/agent/keys"), Admin.AgentKeyList.serializer()).map { it.items }

    suspend fun deleteAgentKey(id: Long): Outcome<String> = delete("/api/agent/keys/$id")

    suspend fun suppressions(): Outcome<List<String>> =
        one(get("/api/admin/suppressions"), Admin.SuppressionList.serializer()).map { it.items }

    /** `allowed` is the whitelist, `blocked` the blacklist. */
    suspend fun senderList(allowed: Boolean): Outcome<List<String>> =
        one(get(senderListPath(allowed)), Admin.SenderList.serializer()).map { it.entries }

    suspend fun addToSenderList(allowed: Boolean, address: String): Outcome<String> = post(
        url(senderListPath(allowed)),
        json.encodeToString(Admin.AddSenderRequest.serializer(), Admin.AddSenderRequest(address)),
        authorized = true,
    )

    suspend fun removeFromSenderList(allowed: Boolean, address: String): Outcome<String> =
        delete(senderListPath(allowed) + "/" + enc(address))

    private fun senderListPath(allowed: Boolean) =
        if (allowed) "/api/spam/whitelist" else "/api/spam/blacklist"

    suspend fun addAlias(req: Admin.AddAliasRequest): Outcome<String> = post(
        url("/api/admin/aliases"),
        json.encodeToString(Admin.AddAliasRequest.serializer(), req),
        authorized = true,
    )

    suspend fun deleteAlias(id: Long): Outcome<String> = delete("/api/admin/aliases/$id")

    suspend fun addDomain(name: String): Outcome<String> = post(
        url("/api/admin/domains"),
        json.encodeToString(Admin.AddDomainRequest.serializer(), Admin.AddDomainRequest(name)),
        authorized = true,
    )

    suspend fun deleteDomain(name: String): Outcome<String> = delete("/api/admin/domains/" + enc(name))

    private suspend fun delete(path: String): Outcome<String> = withContext(Dispatchers.IO) {
        val s = session ?: return@withContext Outcome.Err("Not signed in.")
        send(
            Request.Builder()
                .url(s.server + path)
                .header("Authorization", "Bearer ${s.token}")
                .delete()
                .build(),
        )
    }

    /** Decode one object, where [decode] decodes an array of them. */
    private fun <T> one(
        r: Outcome<String>,
        serializer: kotlinx.serialization.KSerializer<T>,
    ): Outcome<T> = when (r) {
        is Outcome.Ok -> runCatching { json.decodeFromString(serializer, r.value) }.fold(
            onSuccess = { Outcome.Ok(it) },
            onFailure = { Outcome.Err("The server sent a shape this app could not read: ${it.message}") },
        )
        is Outcome.Err -> r
    }

    private fun <T, R> Outcome<T>.map(f: (T) -> R): Outcome<R> = when (this) {
        is Outcome.Ok -> Outcome.Ok(f(value))
        is Outcome.Err -> this
    }

    private fun <T> decode(
        r: Outcome<String>,
        element: kotlinx.serialization.KSerializer<T>,
    ): Outcome<List<T>> = when (r) {
        is Outcome.Ok -> runCatching {
            json.decodeFromString(kotlinx.serialization.builtins.ListSerializer(element), r.value)
        }.fold(
            onSuccess = { Outcome.Ok(it) },
            // Say which shape disagreed. A bare "parse error" here is
            // what made nine wire schemas drift unnoticed on the web.
            onFailure = { Outcome.Err("The server sent a shape this app could not read: ${it.message}") },
        )
        is Outcome.Err -> r
    }

    private suspend fun get(path: String): Outcome<String> = withContext(Dispatchers.IO) {
        val s = session ?: return@withContext Outcome.Err("Not signed in.")
        send(Request.Builder().url(s.server + path).header("Authorization", "Bearer ${s.token}").get().build())
    }

    private suspend fun post(url: String, body: String, authorized: Boolean): Outcome<String> =
        withContext(Dispatchers.IO) {
            val b = Request.Builder().url(url).post(body.toRequestBody(JSON_MEDIA))
            if (authorized) {
                val s = session ?: return@withContext Outcome.Err("Not signed in.")
                b.header("Authorization", "Bearer ${s.token}")
            }
            send(b.build())
        }

    private fun send(request: Request): Outcome<String> = try {
        http.newCall(request).execute().use { response ->
            val text = response.body.string()
            when {
                response.isSuccessful -> Outcome.Ok(text)
                response.code == 401 -> Outcome.Err("Signed out — the server rejected this session.")
                else -> Outcome.Err("The server answered ${response.code}.")
            }
        }
    } catch (e: IOException) {
        // Distinguished from a server error on purpose: one is worth
        // retrying where you are, the other is not.
        Outcome.Err("Could not reach the server: ${e.message}")
    }

    private fun url(path: String) = (session?.server ?: "") + path

    private fun enc(s: String) = java.net.URLEncoder.encode(s, "UTF-8")

    /** `mail.golia.jp` and `https://mail.golia.jp/` mean the same thing. */
    private fun normalise(server: String): String {
        val trimmed = server.trim().removeSuffix("/")
        return if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) trimmed
        else "https://$trimmed"
    }

    /** Success, or something to say to the reader. */
    sealed interface Outcome<out T> {
        data class Ok<T>(val value: T) : Outcome<T>
        data class Err(val message: String) : Outcome<Nothing>
    }

    private companion object {
        val JSON_MEDIA = "application/json; charset=utf-8".toMediaType()
    }
}
