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

    internal val http = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    internal val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

    var session: TokenStore.Session? = store.read()
        private set

    /**
     * Told when the server stops accepting this session.
     *
     * A 401 used to become a sentence and nothing else: the app went on
     * believing it was signed in, every refresh failed with the same
     * words, and the only way out was to find Sign out in Settings. The
     * token is cleared here, once, wherever the 401 arrived — a check
     * at every call site is a check somebody forgets to add to the
     * next one — and this hands the fact to the view model so a screen
     * can ask for the password again.
     */
    var onSessionRejected: (() -> Unit)? = null

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
        set(value) {
            field = value
            // **And the live session follows it.** It used to apply
            // only at login, so pointing the app at another server
            // after signing in changed nothing and every request still
            // went to the old one — which made two tests that thought
            // they had taken the network away pass while talking to it.
            // "Point this app at this server" has to mean every
            // request, or it does not mean anything.
            val current = session ?: return
            if (value == null || value == current.server) return
            session = TokenStore.Session(value, current.token)
        }

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

    /**
     * @param before keyset page boundary — see [ThreadPage] for why it
     *   is one second past the oldest row rather than the oldest row.
     */
    suspend fun conversations(list: MailList, before: Long? = null): Outcome<List<Wire.Conversation>> =
        conversations(list.axes, before)

    suspend fun conversations(axes: MailListAxes, before: Long? = null): Outcome<List<Wire.Conversation>> = decode(
        get(
            "/api/conversations?limit=50&" + axes.query() +
                (before?.let { "&before_ts=$it" } ?: ""),
        ),
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
        get("/api/conversations/search?q=" + enc(term) + "&limit=$SEARCH_LIMIT&" + list.axes.query()),
        Wire.Conversation.serializer(),
    )

    /**
     * `GET /api/conversations/unseen-count` — `{"count": N}`.
     *
     * A number rather than a list: the periodic check wants to know
     * whether anything arrived, and downloading a mailbox in the
     * background to find out would be the wrong trade on a phone.
     */
    suspend fun unseenCount(): Outcome<Int> =
        one(get("/api/conversations/unseen-count"), Wire.UnseenCount.serializer()).map { it.count }


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
        forwardAttachmentsFrom: Int? = null,
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
                forwardAttachmentsFrom = forwardAttachmentsFrom,
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
            // **200 is not success.** The route applies each verb in
            // turn and says in the body how many did not go through. A
            // client that read only the status code takes a partial
            // failure for a clean one and leaves rows off the screen
            // that are still in the mailbox.
            is Outcome.Ok -> runCatching {
                json.decodeFromString(Wire.BatchResult.serializer(), r.value)
            }.fold(
                onSuccess = { result ->
                    if (result.success && result.failed == 0) {
                        Outcome.Ok(Unit)
                    } else {
                        Outcome.Err(
                            result.message
                                ?: "The server refused ${result.failed} of ${threadIds.size}.",
                        )
                    }
                },
                // An answer this app cannot read is not an answer it
                // may call success.
                onFailure = { Outcome.Err("The server sent a shape this app could not read: ${it.message}") },
            )
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

    /** Decode one object, where [decode] decodes an array of them. */
    internal fun <T> one(
        r: Outcome<String>,
        serializer: kotlinx.serialization.KSerializer<T>,
    ): Outcome<T> = when (r) {
        is Outcome.Ok -> runCatching { json.decodeFromString(serializer, r.value) }.fold(
            onSuccess = { Outcome.Ok(it) },
            onFailure = { Outcome.Err("The server sent a shape this app could not read: ${it.message}") },
        )
        is Outcome.Err -> r
    }

    internal fun <T, R> Outcome<T>.map(f: (T) -> R): Outcome<R> = when (this) {
        is Outcome.Ok -> Outcome.Ok(f(value))
        is Outcome.Err -> this
    }

    internal fun <T> decode(
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

    internal suspend fun get(path: String): Outcome<String> = withContext(Dispatchers.IO) {
        val s = session ?: return@withContext Outcome.Err("Not signed in.")
        send(Request.Builder().url(s.server + path).header("Authorization", "Bearer ${s.token}").get().build())
    }

    internal suspend fun post(url: String, body: String, authorized: Boolean): Outcome<String> =
        withContext(Dispatchers.IO) {
            val b = Request.Builder().url(url).post(body.toRequestBody(JSON_MEDIA))
            if (authorized) {
                val s = session ?: return@withContext Outcome.Err("Not signed in.")
                b.header("Authorization", "Bearer ${s.token}")
            }
            send(b.build())
        }

    internal fun send(request: Request): Outcome<String> = try {
        http.newCall(request).execute().use { response ->
            val text = response.body.string()
            when {
                response.isSuccessful -> Outcome.Ok(text)
                response.code == 401 -> {
                    rejected()
                    Outcome.Err("Signed out — the server rejected this session.")
                }
                else -> Outcome.Err("The server answered ${response.code}.")
            }
        }
    } catch (e: IOException) {
        // Distinguished from a server error on purpose: one is worth
        // retrying where you are, the other is not.
        Outcome.Err("Could not reach the server: ${e.message}")
    }

    /**
     * The session is gone; say so once and forget the token.
     *
     * Guarded because a screen makes several requests at a time — the
     * list, the signature, the unseen count — and three 401s in the
     * same second should not be three trips back to the sign-in screen.
     */
    internal fun rejected() {
        if (session == null) return
        session = null
        store.clear()
        onSessionRejected?.invoke()
    }

    internal fun url(path: String) = (session?.server ?: "") + path

    internal fun enc(s: String) = java.net.URLEncoder.encode(s, "UTF-8")

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

    companion object {
        private val JSON_MEDIA = "application/json; charset=utf-8".toMediaType()

        /**
         * What the search endpoint is asked for.
         *
         * It has no keyset parameter — unlike the conversation list,
         * there is no way to ask for the next page — so this is a
         * ceiling and the screen has to say when it was reached. Sent
         * explicitly rather than left to the server's default, because
         * a screen cannot say "the first fifty" about a number it does
         * not know.
         */
        const val SEARCH_LIMIT = 50
    }
}
