package jp.golia.mailrs.accounts

import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.Writer
import java.net.Socket
import javax.net.ssl.SSLSocket
import javax.net.ssl.SSLSocketFactory
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * A conversation with an IMAP server.
 *
 * The socket half. Everything it reads is handed to [Imap] to
 * interpret, so this file is about bytes and timeouts and that file is
 * about grammar — and only one of them needs a server to test.
 *
 * **TLS from the first byte.** No plaintext, and no STARTTLS: a
 * credential is not sent over a connection that was ever in the clear,
 * and every provider worth connecting to offers 993.
 */
class ImapSession(private val host: String, private val port: Int) : AutoCloseable {
    sealed class Failure(message: String) : Exception(message) {
        /** Could not reach the server at all. */
        class Unreachable(val why: String) : Failure(why)

        /**
         * The credential was refused. **Not a network problem** — one
         * is a button to press, the other is waiting.
         */
        class Refused(val detail: String) : Failure(detail)

        /** The server said no to something else. */
        class Server(val detail: String) : Failure(detail)

        /** The connection broke mid-conversation. */
        class Closed : Failure("the server closed the connection")
    }

    private var socket: SSLSocket? = null
    private var reader: BufferedReader? = null
    private var writer: Writer? = null
    private var tag = 0

    /** Open the connection and read the greeting. */
    suspend fun connect(timeoutMs: Int = 20_000) = withContext(Dispatchers.IO) {
        try {
            val plain = Socket()
            plain.connect(java.net.InetSocketAddress(host, port), timeoutMs)
            plain.soTimeout = 60_000
            val s = (SSLSocketFactory.getDefault() as SSLSocketFactory)
                .createSocket(plain, host, port, true) as SSLSocket
            // Names are checked against the certificate. Without this
            // any certificate a machine on the path can produce is
            // accepted, which is the whole of the protection gone.
            s.sslParameters = s.sslParameters.apply { endpointIdentificationAlgorithm = "HTTPS" }
            s.startHandshake()
            socket = s
            reader = BufferedReader(InputStreamReader(s.inputStream, Charsets.UTF_8))
            writer = s.outputStream.writer(Charsets.UTF_8)
        } catch (e: Exception) {
            throw Failure.Unreachable(e.message ?: e.toString())
        }
        val greeting = readLine()
        if (greeting.uppercase().startsWith("* BYE")) {
            throw Failure.Server(greeting.removePrefix("* "))
        }
    }

    override fun close() {
        runCatching { socket?.close() }
        socket = null
    }

    /**
     * Sign in.
     *
     * The password is quoted: generated app passwords contain `"` and
     * `\` often enough that an unquoted LOGIN turns one into a syntax
     * error — and the person is told their password is wrong when it
     * is right.
     */
    suspend fun login(user: String, password: String) = withContext(Dispatchers.IO) {
        val (_, done) = command("LOGIN ${Imap.quoted(user)} ${Imap.quoted(password)}")
        when (done) {
            is Imap.Completion.Ok -> Unit
            is Imap.Completion.No ->
                throw if (Imap.isAuthenticationFailure(done.detail)) {
                    Failure.Refused(done.detail)
                } else {
                    Failure.Server(done.detail)
                }
            is Imap.Completion.Bad -> throw Failure.Server(done.detail)
        }
    }

    /**
     * Sign in with an access token.
     *
     * A provider that rejects the token answers a continuation (`+`)
     * with a base64 error rather than a final code, and will not send
     * one until the client sends an empty line. Reading that `+` as
     * success authenticates every refused token.
     */
    suspend fun authenticateXOAuth2(user: String, token: String) = withContext(Dispatchers.IO) {
        val t = nextTag()
        write("$t AUTHENTICATE XOAUTH2 ${Smtp.authXOAuth2(user, token)}\r\n")
        while (true) {
            val line = readLine()
            if (line.startsWith("+")) {
                write("\r\n")
                continue
            }
            when (val done = Imap.completion(line, t)) {
                is Imap.Completion.Ok -> return@withContext
                is Imap.Completion.No -> throw Failure.Refused(done.detail)
                is Imap.Completion.Bad -> throw Failure.Server(done.detail)
                null -> Unit
            }
        }
    }

    /** Every folder the server offers. */
    suspend fun list(): List<Imap.Untagged.ListFolder> = withContext(Dispatchers.IO) {
        val (untagged, done) = command("""LIST "" "*"""")
        refuseIfNotOk(done)
        untagged.filterIsInstance<Imap.Untagged.ListFolder>()
    }

    /** Open a folder, and say what state it is in. */
    suspend fun select(folder: String): Pair<Long, Int> = withContext(Dispatchers.IO) {
        val (untagged, done) = command("SELECT ${Imap.quoted(folder)}")
        refuseIfNotOk(done)
        var validity = 0L
        var exists = 0
        for (u in untagged) {
            when (u) {
                is Imap.Untagged.UidValidity -> validity = u.value
                is Imap.Untagged.Exists -> exists = u.count
                else -> Unit
            }
        }
        validity to exists
    }

    private fun refuseIfNotOk(done: Imap.Completion) {
        when (done) {
            is Imap.Completion.No -> throw Failure.Server(done.detail)
            is Imap.Completion.Bad -> throw Failure.Server(done.detail)
            is Imap.Completion.Ok -> Unit
        }
    }

    /** One message, as far as a list row needs it. */
    data class Fetched(
        val uid: Long,
        val seen: Boolean,
        val headers: MessageHeaders.Parsed,
        /**
         * Seconds since the epoch, or null when the `Date:` header
         * could not be read. **Null, not now** — a message shown as
         * having just arrived jumps to the top and stays there.
         */
        val date: Long?,
    )

    /**
     * Read the headers of everything in [range].
     *
     * `BODY.PEEK[HEADER]`, not `BODY[HEADER]`: the second marks every
     * message read on the server just for having been listed, which is
     * the rudest thing a mail client can do to somebody's mailbox.
     *
     * The literal is read **by the byte count the server announced**,
     * never by scanning for a terminator: a message contains every
     * byte sequence a terminator could be made of.
     */
    suspend fun fetchHeaders(range: String): List<Fetched> = withContext(Dispatchers.IO) {
        val t = nextTag()
        write("$t UID FETCH $range (UID FLAGS BODY.PEEK[HEADER])\r\n")
        val out = mutableListOf<Fetched>()
        while (true) {
            val line = readLine()
            Imap.completion(line, t)?.let { done ->
                when (done) {
                    is Imap.Completion.Ok -> return@withContext out
                    is Imap.Completion.No -> throw Failure.Server(done.detail)
                    is Imap.Completion.Bad -> throw Failure.Server(done.detail)
                }
            }
            val announced = Imap.fetchLine(line) ?: continue
            val uid = announced.uid ?: continue
            // A flags-only reply: nothing to read, and nothing a row
            // can show that it does not already have.
            val count = announced.literalBytes ?: continue
            val raw = readBytes(count)
            val headers = MessageHeaders.parse(raw)
            out += Fetched(uid, announced.seen, headers, MailDate.epochSeconds(headers.date))
        }
        @Suppress("UNREACHABLE_CODE")
        out
    }

    /** Exactly [count] bytes, whatever they contain. */
    private fun readBytes(count: Int): String {
        val buf = CharArray(count)
        var read = 0
        val r = reader ?: throw Failure.Closed()
        while (read < count) {
            val n = r.read(buf, read, count - read)
            if (n < 0) throw Failure.Closed()
            read += n
        }
        return String(buf, 0, read)
    }

    // MARK: the wire

    private fun nextTag(): String = "a${++tag}"

    /** Send a command and read until its tagged reply. */
    private fun command(text: String): Pair<List<Imap.Untagged>, Imap.Completion> {
        val t = nextTag()
        write("$t $text\r\n")
        val untagged = mutableListOf<Imap.Untagged>()
        while (true) {
            val line = readLine()
            Imap.completion(line, t)?.let { return untagged to it }
            Imap.untagged(line)?.let { untagged += it }
        }
    }

    private fun write(text: String) {
        val w = writer ?: throw Failure.Closed()
        w.write(text)
        w.flush()
    }

    private fun readLine(): String =
        reader?.readLine() ?: throw Failure.Closed()
}
