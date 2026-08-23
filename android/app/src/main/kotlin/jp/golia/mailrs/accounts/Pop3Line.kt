package jp.golia.mailrs.accounts

/**
 * Reading what a POP3 server says.
 *
 * POP3 has no response codes and no tags: every reply is `+OK` or
 * `-ERR` and the rest of the line is free text. That makes two things
 * this client has to be careful about, and both are here rather than
 * in the socket.
 */
object Pop3 {
    data class Reply(val ok: Boolean, val text: String)

    fun reply(line: String): Reply? {
        val t = line.trim()
        return when {
            t.startsWith("+OK") -> Reply(true, t.removePrefix("+OK").trim())
            t.startsWith("-ERR") -> Reply(false, t.removePrefix("-ERR").trim())
            else -> null
        }
    }

    /**
     * `UIDL` — the only durable identity POP3 offers.
     *
     * Message **numbers** are renumbered on every session: message 3
     * today is a different message tomorrow. Anything that remembers
     * what has been seen has to remember the uidl, and a client that
     * remembers numbers re-downloads the mailbox after every delete
     * somebody makes elsewhere.
     */
    data class Uidl(val number: Int, val id: String)

    /** One line of a `UIDL` listing: `3 QhdPYR:00WBw1Ph7x7`. */
    fun uidl(line: String): Uidl? {
        val t = line.trim()
        val space = t.indexOf(' ')
        if (space <= 0) return null
        val n = t.substring(0, space).toIntOrNull() ?: return null
        val id = t.substring(space + 1).trim()
        return if (id.isEmpty()) null else Uidl(n, id)
    }

    /**
     * Undo the dot-stuffing a server applies to a retrieved message.
     *
     * The mirror of what SMTP does on the way out: a body line that
     * began with `.` arrives doubled, and a client that does not undo
     * it corrupts every message containing such a line. `.` alone on a
     * line ends the response and is not part of the message.
     */
    fun unstuffed(lines: List<String>): String =
        lines.takeWhile { it != "." }
            .joinToString("\r\n") { if (it.startsWith("..")) it.drop(1) else it }

    /**
     * Whether a refusal means the credential is wrong.
     *
     * POP3 has no code for it, so the words are all there is.
     */
    fun isAuthenticationFailure(text: String): Boolean {
        val t = text.uppercase()
        return t.contains("AUTHENTICATION FAILED") ||
            (t.contains("INVALID") && t.contains("PASSWORD")) ||
            t.contains("LOGIN FAILED") ||
            (t.contains("AUTH") && t.contains("FAIL"))
    }
}
