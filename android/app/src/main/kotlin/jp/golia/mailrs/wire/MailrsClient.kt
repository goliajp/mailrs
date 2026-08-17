package jp.golia.mailrs.wire

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
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

    suspend fun conversations(): Outcome<List<Wire.Conversation>> = decode(
        get("/api/conversations?limit=50"),
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
    suspend fun search(term: String): Outcome<List<Wire.Conversation>> = decode(
        get("/api/conversations/search?q=" + java.net.URLEncoder.encode(term, "UTF-8")),
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
    ): Outcome<Unit> {
        val payload = json.encodeToString(
            Wire.SendRequest.serializer(),
            Wire.SendRequest(to = to, subject = subject, body = body, inReplyTo = inReplyTo),
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

    enum class Verb(val wire: String) {
        Archive("archive"),
        Unarchive("unarchive"),
        Read("read"),
        Unread("unread"),
    }

    /** Mark a thread read. Best-effort: the list still works if it fails. */
    suspend fun markRead(threadId: String) {
        post(url("/api/conversations/${enc(threadId)}/read"), "{}", authorized = true)
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
