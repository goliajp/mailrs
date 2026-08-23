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
 * A POP3 conversation.
 *
 * POP3 is not a smaller IMAP; it is a different arrangement, and two
 * differences decide everything this session does:
 *
 * - **There are no folders.** A POP3 account has one mailbox, and
 *   anything a person filed elsewhere is not visible here at all.
 * - **There are no server-side flags and no stable numbers.** Message 3
 *   today is a different message tomorrow, so `UIDL` is the only
 *   durable identity, and read state can only be kept on this device.
 */
class Pop3Session(private val host: String, private val port: Int) : AutoCloseable {
    sealed class Failure(message: String) : Exception(message) {
        class Unreachable(val why: String) : Failure(why)
        /** The credential was refused. */
        class Refused(val detail: String) : Failure(detail)
        /** The server said no to something else. */
        class Server(val detail: String) : Failure(detail)
        class Closed : Failure("the server closed the connection")
    }

    /** Where the bytes go. A seam, so the conversation can be tested. */
    interface Transport {
        fun readLine(): String
        fun write(text: String)
        fun close()
    }

    internal var transport: Transport? = null
    private var socket: SSLSocket? = null
    private var reader: BufferedReader? = null
    private var writer: Writer? = null

    /** Open the connection and read the greeting. */
    suspend fun connect(timeoutMs: Int = 20_000) = withContext(Dispatchers.IO) {
        if (transport == null) {
            try {
                val plain = Socket()
                plain.connect(InetSocketAddress(host, port), timeoutMs)
                plain.soTimeout = 60_000
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
                // Byte-preserving, for the same reason as IMAP: a
                // message declares its own charset, and decoding here
                // settles it before the header saying so has been read.
                reader = BufferedReader(InputStreamReader(s.inputStream, Charsets.ISO_8859_1))
                writer = s.outputStream.writer(Charsets.UTF_8)
            } catch (e: Exception) {
                throw Failure.Unreachable(e.message ?: e.toString())
            }
        }
        expect(readReply())
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
     * `USER` then `PASS`, and **both** answers matter: a server that
     * accepts the name and refuses the password says so only on the
     * second, and a client that checks one of them signs in to nothing.
     */
    suspend fun login(user: String, password: String) = withContext(Dispatchers.IO) {
        val named = command("USER $user")
        if (!named.ok) throw refusal(named.text)
        val secret = command("PASS $password")
        if (!secret.ok) throw refusal(secret.text)
        Unit
    }

    /** Every message in the mailbox, by its durable identity. */
    suspend fun uidls(): List<Pop3.Uidl> = withContext(Dispatchers.IO) {
        val reply = command("UIDL")
        if (!reply.ok) throw Failure.Server(reply.text)
        val out = mutableListOf<Pop3.Uidl>()
        while (true) {
            val line = readLine()
            if (line.trim() == ".") return@withContext out
            Pop3.uidl(line)?.let { out.add(it) }
        }
        @Suppress("UNREACHABLE_CODE")
        out
    }

    /**
     * The headers of one message, without downloading it.
     *
     * `TOP n 0` — the headers and none of the body. A list that fetched
     * whole messages would download the mailbox to show a list, and on
     * a phone that is somebody's data allowance.
     */
    suspend fun headers(number: Int): String = withContext(Dispatchers.IO) {
        val reply = command("TOP $number 0")
        if (!reply.ok) throw Failure.Server(reply.text)
        Pop3.unstuffed(readUntilDot())
    }

    /** One whole message. */
    suspend fun retrieve(number: Int): ByteArray = withContext(Dispatchers.IO) {
        val reply = command("RETR $number")
        if (!reply.ok) throw Failure.Server(reply.text)
        Wire.bytes(Pop3.unstuffed(readUntilDot()))
    }

    /**
     * Mark a message for deletion.
     *
     * **`DELE` does not delete anything.** RFC 1939 marks the message
     * and the server only acts on it when the session ends with
     * `QUIT` — a session dropped instead leaves the mailbox untouched,
     * which is a delete that silently did not happen. The pairing is
     * the caller's to get right, and it is asserted.
     */
    suspend fun delete(number: Int) = withContext(Dispatchers.IO) {
        val reply = command("DELE $number")
        if (!reply.ok) throw Failure.Server(reply.text)
        Unit
    }

    /**
     * End the session properly.
     *
     * `QUIT` is not politeness: a POP3 server holds an exclusive lock
     * on the mailbox for the length of a session, and one dropped
     * without QUIT keeps that lock until it times out — during which
     * nothing else, including the person's other device, can read their
     * mail.
     */
    suspend fun quit() = withContext(Dispatchers.IO) {
        runCatching { command("QUIT") }
        Unit
    }

    private fun readUntilDot(): List<String> {
        val out = mutableListOf<String>()
        while (true) {
            val line = readLine()
            out.add(line)
            if (line.trim() == ".") return out
        }
    }

    private fun refusal(text: String): Failure = when {
        Pop3.isAuthenticationFailure(text) -> Failure.Refused(text)
        else -> Failure.Server(text)
    }

    private fun expect(reply: Pop3.Reply) {
        if (!reply.ok) throw Failure.Server(reply.text)
    }

    private fun command(text: String): Pop3.Reply {
        write(text + "\r\n")
        return readReply()
    }

    private fun readReply(): Pop3.Reply =
        Pop3.reply(readLine()) ?: throw Failure.Server("the server did not answer in POP3")

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
