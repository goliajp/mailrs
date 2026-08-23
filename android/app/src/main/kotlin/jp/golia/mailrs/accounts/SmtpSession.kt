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

    private var socket: SSLSocket? = null
    private var reader: BufferedReader? = null
    private var writer: Writer? = null

    suspend fun connect(helo: String, timeoutMs: Int = 20_000) = withContext(Dispatchers.IO) {
        // Only implicit TLS for now. **No downgrade**: a server that
        // will not encrypt from the first byte is a server this
        // credential does not go to, and STARTTLS on 587 is a second
        // path that has to be written and tested rather than assumed.
        if (port != 465) {
            throw Failure.Rejected(0, "only port 465 is supported yet", true)
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

    override fun close() {
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
            if (reply.isPositive) return@withContext
            if (reply.code == 334) {
                val final = command("")
                throw Failure.Refused(final.text)
            }
            throw if (Smtp.isAuthenticationFailure(reply.code, reply.text)) {
                Failure.Refused(reply.text)
            } else {
                Failure.Rejected(reply.code, reply.text, reply.isPermanent)
            }
        }

    /** Hand one message over. */
    suspend fun send(from: String, to: List<String>, message: String) =
        withContext(Dispatchers.IO) {
            expect(command("MAIL FROM:<$from>"))
            for (rcpt in to) expect(command("RCPT TO:<$rcpt>"))
            val start = command("DATA")
            if (start.code != 354) {
                throw Failure.Rejected(start.code, start.text, start.isPermanent)
            }
            // Dot-stuffed: a body line beginning with `.` would end the
            // block here and the message would arrive cut in half.
            write(Smtp.dotStuffed(message) + "\r\n.\r\n")
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
        while (true) {
            val line = reader?.readLine() ?: throw Failure.Closed()
            val r = Smtp.reply(line) ?: continue
            if (!r.more) return r
        }
    }

    private fun write(text: String) {
        val w = writer ?: throw Failure.Closed()
        w.write(text)
        w.flush()
    }
}
