package jp.golia.mailrs.accounts

import java.net.HttpURLConnection
import java.net.URL
import java.util.Base64
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Talking to a JMAP server.
 *
 * Ordinary HTTPS, so there is no socket conversation to get wrong — the
 * shapes are in [Jmap], which needs nothing at all. What is here is the
 * two requests and the seam that lets them be tested: finding the
 * session object, then asking for mail.
 */
class JmapSession(private val host: String) {
    sealed class Failure(message: String) : Exception(message) {
        class Unreachable(val why: String) : Failure(why)
        /** The credential was refused — 401, or a session that will not load. */
        class Refused(val detail: String) : Failure(detail)
        class Server(val detail: String) : Failure(detail)
    }

    /** One HTTPS exchange. A seam, so the requests can be tested. */
    interface Http {
        /** @return status and body. */
        fun post(url: String, authorization: String, body: String?): Pair<Int, String>
    }

    internal var http: Http? = null

    /**
     * Find the API url and the account id.
     *
     * `/.well-known/jmap` is the only entry point a client may assume;
     * everything else about a server comes out of what it answers here.
     */
    suspend fun session(user: String, secret: String): Jmap.Session =
        withContext(Dispatchers.IO) {
            val (status, body) = exchange(
                "https://$host/.well-known/jmap", authorization(user, secret), null,
            )
            if (status == 401 || status == 403) {
                throw Failure.Refused("the server refused this account's credential")
            }
            if (status !in 200..299) throw Failure.Server("the server answered $status")
            Jmap.session(body)
                ?: throw Failure.Server("the server did not say which account holds the mail")
        }

    /** The newest messages, in one round trip. */
    suspend fun newest(
        session: Jmap.Session,
        user: String,
        secret: String,
        limit: Int = 50,
    ): List<Jmap.Email> = withContext(Dispatchers.IO) {
        val (status, body) = exchange(
            session.apiUrl,
            authorization(user, secret),
            Jmap.newestRequest(session.accountId, limit),
        )
        if (status == 401 || status == 403) {
            throw Failure.Refused("the server refused this account's credential")
        }
        if (status !in 200..299) throw Failure.Server("the server answered $status")
        Jmap.emails(body) ?: throw Failure.Server("the server's answer could not be read")
    }

    /**
     * Basic for a password, Bearer for a token.
     *
     * Sending a token as a password is refused by every server that
     * issues tokens — and the person is then told their password is
     * wrong for an account whose credentials are fine.
     */
    internal fun authorization(user: String, secret: String): String {
        if (user.isEmpty()) return "Bearer $secret"
        val pair = "$user:$secret".toByteArray(Charsets.UTF_8)
        return "Basic " + Base64.getEncoder().encodeToString(pair)
    }

    private fun exchange(url: String, authorization: String, body: String?): Pair<Int, String> {
        http?.let { return it.post(url, authorization, body) }
        return try {
            val connection = URL(url).openConnection() as HttpURLConnection
            connection.requestMethod = when (body) {
                null -> "GET"
                else -> "POST"
            }
            connection.setRequestProperty("Authorization", authorization)
            connection.setRequestProperty("Accept", "application/json")
            connection.connectTimeout = 20_000
            connection.readTimeout = 60_000
            if (body != null) {
                connection.setRequestProperty("Content-Type", "application/json")
                connection.doOutput = true
                connection.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
            }
            val status = connection.responseCode
            // The error stream, not the input stream, once the status is
            // a failure: reading the wrong one throws and the reason the
            // server gave is lost.
            val stream = when {
                status in 200..299 -> connection.inputStream
                else -> connection.errorStream
            }
            val text = stream?.bufferedReader(Charsets.UTF_8)?.use { it.readText() }.orEmpty()
            connection.disconnect()
            status to text
        } catch (e: Exception) {
            throw Failure.Unreachable(e.message ?: e.toString())
        }
    }
}
