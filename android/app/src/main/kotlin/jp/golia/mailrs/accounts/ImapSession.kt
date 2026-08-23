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

    /**
     * Where the bytes go.
     *
     * A seam, so the conversation above it can be tested without a
     * server. Every rule this session enforces — a tag prefix that must
     * not match `a10` when it asked as `a1`, a literal read by the byte
     * count the server announced, a folder name at the end of a LIST
     * line — is a rule about what arrives, and none of them can be
     * checked by connecting to a real server and hoping it sends the
     * awkward case.
     */
    interface Transport {
        fun readLine(): String
        fun readBytes(count: Int): String
        fun write(text: String)
        fun close()
    }

    /** Swapped in by tests; null means the real socket. */
    internal var transport: Transport? = null

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
            // **ISO-8859-1, deliberately.** It maps every byte to the
            // code point of the same value, so what is read back is
            // exactly what arrived and nothing has been decided yet. A
            // UTF-8 reader here settles a message's charset before the
            // header declaring it has been read, and every Shift_JIS
            // and windows-1252 body arrives as replacement characters.
            // `Wire.utf8` turns it back into text where text is what is
            // wanted.
            reader = BufferedReader(InputStreamReader(s.inputStream, Charsets.ISO_8859_1))
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
        transport?.let {
            runCatching { it.close() }
            return
        }
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
            val headers = MessageHeaders.parse(Wire.utf8(readBytes(count)))
            out += Fetched(uid, announced.seen, headers, MailDate.epochSeconds(headers.date))
        }
        @Suppress("UNREACHABLE_CODE")
        out
    }

    /**
     * One message, whole.
     *
     * `BODY.PEEK[]` rather than `BODY[]` for the same reason the header
     * fetch peeks: opening a message is the reader's decision, and a
     * client that marks mail read for having looked at it takes that
     * decision away. Marking read is a separate, deliberate call.
     */
    suspend fun fetchRaw(uid: Long): ByteArray = withContext(Dispatchers.IO) {
        val t = nextTag()
        write("$t UID FETCH $uid (BODY.PEEK[])\r\n")
        var out = ByteArray(0)
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
            val count = announced.literalBytes ?: continue
            // The announced byte count, never a scan for a terminator: a
            // message contains every byte sequence a terminator could be
            // made of.
            out = Wire.bytes(readBytes(count))
        }
        @Suppress("UNREACHABLE_CODE")
        out
    }

    /** Mark a message read on the server, because somebody read it. */
    suspend fun markSeen(uid: Long) = store(uid, "+FLAGS", "\\Seen")

    /**
     * Mark it unread again.
     *
     * `-FLAGS`, not a `FLAGS` that names what should remain: the
     * second replaces the whole set, so it would quietly clear
     * `\Flagged`, `\Answered` and every keyword the person or another
     * client had put there.
     */
    suspend fun markUnseen(uid: Long) = store(uid, "-FLAGS", "\\Seen")

    private suspend fun store(uid: Long, op: String, flag: String) = withContext(Dispatchers.IO) {
        val t = nextTag()
        write("$t UID STORE $uid $op ($flag)\r\n")
        while (true) {
            val line = readLine()
            Imap.completion(line, t)?.let { done ->
                when (done) {
                    is Imap.Completion.Ok -> return@withContext
                    is Imap.Completion.No -> throw Failure.Server(done.detail)
                    is Imap.Completion.Bad -> throw Failure.Server(done.detail)
                }
            }
        }
    }

    /**
     * What the server said it can do.
     *
     * Read rather than assumed: the two commands below exist only on
     * some servers, and asking for one that is not there is an error
     * the person sees rather than a fallback they do not.
     */
    suspend fun capabilities(): Set<String> = withContext(Dispatchers.IO) {
        val (untagged, done) = command("CAPABILITY")
        refuseIfNotOk(done)
        untagged.filterIsInstance<Imap.Untagged.Capabilities>()
            .flatMap { it.names }
            .map { it.uppercase() }
            .toSet()
    }

    /**
     * Put a message in another folder.
     *
     * `MOVE` (RFC 6851) where the server has it, and the older
     * three-step dance where it does not — and the difference matters
     * more than it looks:
     *
     * **A bare `EXPUNGE` removes every message in the folder flagged
     * `\Deleted`**, including ones somebody else's client flagged and
     * has not expunged yet. `UID EXPUNGE` (RFC 4315) removes only the
     * one named. Where neither `MOVE` nor `UIDPLUS` is offered, the
     * message is flagged and **left** rather than expunged: it
     * disappears from the list either way, and no other message is
     * taken with it.
     */
    suspend fun moveTo(uid: Long, folder: String, capabilities: Set<String>) =
        withContext(Dispatchers.IO) {
            if ("MOVE" in capabilities) {
                val t = nextTag()
                write("$t UID MOVE $uid ${Imap.quoted(folder)}\r\n")
                awaitCompletion(t)
                return@withContext
            }
            val copy = nextTag()
            write("$copy UID COPY $uid ${Imap.quoted(folder)}\r\n")
            awaitCompletion(copy)
            store(uid, "+FLAGS", "\\Deleted")
            if ("UIDPLUS" in capabilities) {
                val expunge = nextTag()
                write("$expunge UID EXPUNGE $uid\r\n")
                awaitCompletion(expunge)
            }
        }

    private suspend fun awaitCompletion(tag: String) = withContext(Dispatchers.IO) {
        while (true) {
            val line = readLine()
            Imap.completion(line, tag)?.let { done ->
                when (done) {
                    is Imap.Completion.Ok -> return@withContext
                    is Imap.Completion.No -> throw Failure.Server(done.detail)
                    is Imap.Completion.Bad -> throw Failure.Server(done.detail)
                }
            }
        }
    }

    /** Exactly [count] bytes, whatever they contain. */
    private fun readBytes(count: Int): String {
        transport?.let { return it.readBytes(count) }
        return readBytesFromSocket(count)
    }

    private fun readBytesFromSocket(count: Int): String {
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
        transport?.let {
            it.write(text)
            return
        }
        val w = writer ?: throw Failure.Closed()
        w.write(text)
        w.flush()
    }

    private fun readLine(): String {
        transport?.let { return it.readLine() }
        return reader?.readLine() ?: throw Failure.Closed()
    }
}
