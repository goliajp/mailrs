package jp.golia.mailrs.accounts

import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.Writer
import java.net.InetSocketAddress
import java.net.Socket
import javax.net.ssl.SSLSocket
import javax.net.ssl.SSLSocketFactory
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * A conversation with a submission server.
 *
 * Submission, not delivery: this speaks to the provider's own server
 * on 465 with the account's credential. Sending mail from a phone
 * straight to the recipient's MX fails SPF and DMARC at every
 * receiver, so it is not an option and not a preference.
 */
class SmtpSession(private val host: String, private val port: Int) : AutoCloseable {
    sealed class Failure(message: String) : Exception(message) {
        class Unreachable(val why: String) : Failure(why)

        /** The credential was refused — a button to press. */
        class Refused(val detail: String) : Failure(detail)

        /**
         * The server said no. [permanent] decides whether trying again
         * could ever work.
         */
        class Rejected(val code: Int, val text: String, val permanent: Boolean) : Failure(text)

        class Closed : Failure("the server closed the connection")
    }

    /**
     * Where the bytes go.
     *
     * A seam, so the conversation above it can be tested without a
     * server. The rules this session enforces — the fourth character of
     * a reply is what says whether more is coming, a 334 is a refusal
     * and not a success, a body line beginning with a dot must be
     * stuffed — are rules about what arrives, and a real server is
     * exactly the thing that will not send the awkward case on demand.
     */
    interface Transport {
        fun readLine(): String
        fun write(text: String)
        fun close()
    }

    /** Swapped in by tests; null means the real socket. */
    internal var transport: Transport? = null

    private var socket: SSLSocket? = null
    private var reader: BufferedReader? = null
    private var writer: Writer? = null

    /**
     * Open the connection, encrypt it, and greet.
     *
     * Two ways in, because the world has two: **465 is encrypted from
     * the first byte**, and **587 starts in the clear and upgrades**.
     * Refusing the second is not caution — Outlook and iCloud are
     * 587-only, and the provider table hands out that port, so a
     * session that refuses it is a mailbox that can never send.
     *
     * **No downgrade either way.** If the upgrade does not happen the
     * connection is dropped rather than used: an unencrypted
     * submission carries the password in the clear, and the point of
     * refusing is that nobody can make it happen by standing in the
     * middle and removing STARTTLS from the capability list.
     */
    suspend fun connect(helo: String, timeoutMs: Int = 20_000) = withContext(Dispatchers.IO) {
        if (transport != null) {
            // A scripted server: the conversation is what is under
            // test, and there is no socket to upgrade. The conversation
            // is otherwise the same one, **including the second EHLO**
            // — a test path that skips a command is a test of a
            // different protocol.
            readReply()
            val greeted = command("EHLO $helo")
            if (port != 465) {
                startTls(helo, greeted)
                command("EHLO $helo")
            }
            return@withContext
        }
        if (port != 465) {
            connectWithStartTls(helo, timeoutMs)
            return@withContext
        }
        try {
            val plain = Socket()
            plain.connect(InetSocketAddress(host, port), timeoutMs)
            plain.soTimeout = 60_000
            val s = (SSLSocketFactory.getDefault() as SSLSocketFactory)
                .createSocket(plain, host, port, true) as SSLSocket
            s.sslParameters = s.sslParameters.apply { endpointIdentificationAlgorithm = "HTTPS" }
            s.startHandshake()
            socket = s
            reader = BufferedReader(InputStreamReader(s.inputStream, Charsets.UTF_8))
            writer = s.outputStream.writer(Charsets.UTF_8)
        } catch (e: Exception) {
            throw Failure.Unreachable(e.message ?: e.toString())
        }
        readReply()
        command("EHLO $helo")
    }

    /** 587 and friends: greet in the clear, upgrade, greet again. */
    private fun connectWithStartTls(helo: String, timeoutMs: Int) {
        val plain: Socket
        try {
            plain = Socket()
            plain.connect(InetSocketAddress(host, port), timeoutMs)
            plain.soTimeout = 60_000
            reader = BufferedReader(InputStreamReader(plain.inputStream, Charsets.ISO_8859_1))
            writer = plain.outputStream.writer(Charsets.UTF_8)
        } catch (e: Exception) {
            throw Failure.Unreachable(e.message ?: e.toString())
        }
        readReply()
        val greeted = command("EHLO $helo")
        startTls(helo, greeted)
        try {
            val s = (SSLSocketFactory.getDefault() as SSLSocketFactory)
                .createSocket(plain, host, port, true) as SSLSocket
            // Names are checked against the certificate. Without this
            // any certificate a machine on the path can produce is
            // accepted, which is the whole of the protection gone.
            s.sslParameters = s.sslParameters.apply {
                endpointIdentificationAlgorithm = "HTTPS"
            }
            s.startHandshake()
            socket = s
            reader = BufferedReader(InputStreamReader(s.inputStream, Charsets.ISO_8859_1))
            writer = s.outputStream.writer(Charsets.UTF_8)
        } catch (e: Exception) {
            runCatching { plain.close() }
            throw Failure.Unreachable(e.message ?: e.toString())
        }
        // Again, because everything the server said before the upgrade
        // was said by whoever was on the wire at the time.
        command("EHLO $helo")
    }

    /**
     * Ask for the upgrade, and refuse to go on without it.
     *
     * The capability list is read from the greeting rather than
     * trusted: a server that does not offer STARTTLS is not asked, and
     * one that offers it and then refuses is dropped. Neither case
     * continues in the clear.
     */
    private fun startTls(helo: String, greeted: Smtp.Reply) {
        if (!greeted.text.uppercase().contains("STARTTLS")) {
            throw Failure.Refused("the server did not offer to encrypt the connection")
        }
        val upgrade = command("STARTTLS")
        if (upgrade.code != 220) {
            throw Failure.Refused("the server refused to encrypt the connection")
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
     * Sign in with a password or an access token.
     *
     * A provider that rejects an OAuth token answers 334 with a base64
     * error rather than a final code, and waits for an empty line
     * before sending one. Reading the 334 as success authenticates
     * every refused token.
     */
    suspend fun authenticate(user: String, secret: String, oauth: Boolean) =
        withContext(Dispatchers.IO) {
            val reply = if (oauth) {
                command("AUTH XOAUTH2 ${Smtp.authXOAuth2(user, secret)}")
            } else {
                command("AUTH PLAIN ${Smtp.authPlain(user, secret)}")
            }
            // **334 first.** It is inside the 2xx-3xx range that
            // `isPositive` calls success, so testing that first made
            // this branch unreachable and reported every refused token
            // as signed in — which is precisely what the note above
            // says must not happen. Caught by a scripted server, never
            // by a real one: a real one only sends 334 when a token is
            // genuinely bad.
            if (reply.code == 334) {
                val final = command("")
                throw Failure.Refused(final.text)
            }
            if (reply.isPositive) return@withContext
            throw if (Smtp.isAuthenticationFailure(reply.code, reply.text)) {
                Failure.Refused(reply.text)
            } else {
                Failure.Rejected(reply.code, reply.text, reply.isPermanent)
            }
        }

    /** Hand one message over. */
    suspend fun send(from: String, to: List<String>, message: String) =
        send(from, to, sequenceOf(message))

    /**
     * Hand one message over, in as many pieces as it comes in.
     *
     * **Streamed rather than assembled.** A 25 MB attachment built into
     * one string and dot-stuffed into another is several times its own
     * size in memory at once, and on a phone that is a process the
     * system kills — which looks exactly like mail that vanished. Here
     * each piece is stuffed and written as it arrives, so what is held
     * is one piece.
     *
     * The stuffing is [DotStuffer]'s, not [Smtp.dotStuffed]'s, because
     * a piece can end on the line break whose next line begins with a
     * dot — and a stuffer with no memory of that truncates the message
     * there while it still arrives looking complete.
     */
    suspend fun send(from: String, to: List<String>, message: Sequence<String>) =
        withContext(Dispatchers.IO) {
            expect(command("MAIL FROM:<$from>"))
            for (rcpt in to) expect(command("RCPT TO:<$rcpt>"))
            val start = command("DATA")
            if (start.code != 354) {
                throw Failure.Rejected(start.code, start.text, start.isPermanent)
            }
            val stuffer = DotStuffer()
            var last = ""
            for (piece in message) {
                if (piece.isEmpty()) continue
                write(stuffer.feed(piece))
                last = piece
            }
            // The terminator needs a line of its own, and a message
            // that already ended on one must not gain a blank line —
            // some servers keep it and the reader sees it.
            val terminator = when {
                last.endsWith("\r\n") -> ".\r\n"
                else -> "\r\n.\r\n"
            }
            write(terminator)
            expect(readReply())
            runCatching { command("QUIT") }
            Unit
        }

    // MARK: the wire

    private fun expect(reply: Smtp.Reply) {
        if (!reply.isPositive) {
            throw Failure.Rejected(reply.code, reply.text, reply.isPermanent)
        }
    }

    private fun command(text: String): Smtp.Reply {
        write(text + "\r\n")
        return readReply()
    }

    /**
     * A reply, however many lines it takes.
     *
     * `250-STARTTLS` continues and `250 OK` ends; reading only the
     * first line leaves the rest in the buffer, and the next command
     * then reads somebody else's answer.
     */
    private fun readReply(): Smtp.Reply {
        // **Every line, joined.** A multi-line reply is one reply, and
        // throwing away all but the last discards the EHLO capability
        // list — which is the only place `STARTTLS` is announced, and
        // the only way to know whether an upgrade is even possible. It
        // is also where a server explains a refusal, in the lines
        // before the one carrying the code.
        val gathered = StringBuilder()
        while (true) {
            val line = readLine()
            val r = Smtp.reply(line) ?: continue
            if (gathered.isNotEmpty()) gathered.append('\n')
            gathered.append(r.text)
            if (!r.more) return r.copy(text = gathered.toString())
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
