package jp.golia.mailrs.accounts

/**
 * Getting bytes back out of what the socket reader produced.
 *
 * The IMAP and SMTP readers decode as ISO-8859-1, which maps every byte
 * to the code point of the same value and is therefore lossless and
 * reversible. That is the point: a mail session carries protocol text
 * (ASCII), folder names (usually modified UTF-7, also ASCII) and message
 * bodies (any encoding at all, declared inside the message). Only the
 * last of those can say what it is, so nothing may be decoded until
 * something has read that declaration.
 */
object Wire {
    /** The exact bytes that arrived. */
    fun bytes(s: String): ByteArray = s.toByteArray(Charsets.ISO_8859_1)

    /**
     * Read back as UTF-8, for the places that really are text: header
     * blocks, and folder names on the servers that send them raw.
     *
     * Falls back to what was there when the bytes are not valid UTF-8 —
     * a latin-1 folder name should show as latin-1 rather than as
     * replacement characters.
     */
    fun utf8(s: String): String {
        val raw = bytes(s)
        val decoded = String(raw, Charsets.UTF_8)
        return when {
            decoded.contains('\uFFFD') -> s
            else -> decoded
        }
    }
}
