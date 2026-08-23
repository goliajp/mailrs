package jp.golia.mailrs.accounts

import java.util.Base64

/**
 * Reading what an SMTP server says, and the two AUTH payloads.
 *
 * Split from the socket for the same reason as [Imap], and the same
 * cases as the iOS side: a client that disagrees with itself across
 * platforms is two clients.
 */
object Smtp {
    /** One reply: a code, and whether more lines follow. */
    data class Reply(val code: Int, val text: String, val more: Boolean) {
        val isPositive: Boolean get() = code in 200..399

        /** 4xx is "try later", 5xx is "do not try again". */
        val isPermanent: Boolean get() = code in 500..599
    }

    /**
     * Read one reply line.
     *
     * The fourth character decides: `-` means another line follows, a
     * space means this was the last. Getting that wrong reads the next
     * command's reply as this one's.
     */
    fun reply(line: String): Reply? {
        val t = line.trim()
        if (t.length < 3) return null
        val code = t.take(3).toIntOrNull() ?: return null
        if (t.length == 3) return Reply(code, "", false)
        val sep = t[3]
        if (sep != '-' && sep != ' ') return null
        return Reply(code, t.drop(4), sep == '-')
    }

    /**
     * `AUTH PLAIN` — RFC 4616.
     *
     * Authorisation identity, authentication identity and password,
     * separated by **NUL**, then base64. The separator is the trap:
     * spaces authenticate as nobody and the server answers with what
     * reads as a wrong password. The authorisation identity is left
     * empty — repeating the username there is accepted by some servers
     * and refused by Gmail.
     */
    fun authPlain(user: String, password: String): String {
        val raw = byteArrayOf(0) + user.toByteArray() + byteArrayOf(0) + password.toByteArray()
        return Base64.getEncoder().encodeToString(raw)
    }

    /**
     * `AUTH XOAUTH2`.
     *
     * Not `AUTH PLAIN` with a different secret: SOH separators, an
     * `auth=Bearer ` prefix, and two terminators. An access token sent
     * through `AUTH PLAIN` is refused, and the person is then told
     * their password is wrong for an account whose credentials are
     * perfectly good.
     */
    fun authXOAuth2(user: String, token: String): String =
        Base64.getEncoder().encodeToString(
            "user=$user\u0001auth=Bearer $token\u0001\u0001".toByteArray(),
        )

    /**
     * Dot-stuffing — RFC 5321 4.5.2.
     *
     * A body line beginning with `.` would otherwise end the DATA
     * block, truncating the message at that line. Every mail client
     * has shipped this bug at least once; the symptom is a message
     * that arrives cut in half.
     */
    fun dotStuffed(body: String): String =
        body.replace("\r\n", "\n")
            .split("\n")
            .joinToString("\r\n") { if (it.startsWith(".")) ".$it" else it }

    /**
     * Whether a refusal means the credential is wrong.
     *
     * 535 is the code; some servers only say it in the text. One is a
     * button to press, the other is waiting.
     */
    fun isAuthenticationFailure(code: Int, text: String): Boolean {
        if (code == 535) return true
        val t = text.uppercase()
        return t.contains("AUTHENTICATION FAILED") ||
            t.contains("INVALID CREDENTIALS") ||
            t.contains("USERNAME AND PASSWORD NOT ACCEPTED")
    }
}
