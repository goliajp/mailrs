package jp.golia.mailrs.wire

/**
 * The few lines at the bottom that say who sent this.
 *
 * Ported from `ios/Mailrs/Wire/MailSignature.swift`, including why it
 * comes from the server: the web keeps its signature in
 * `localStorage`, so it belongs to a browser rather than to a person —
 * set it at the desk and the phone, the laptop and a second browser
 * each sign differently, or not at all. The server has had a per-user
 * store the whole time; reading it makes the signature follow the
 * account.
 */
object MailSignature {

    /**
     * `-- ` on a line of its own: RFC 3676 §4.3's separator, and what
     * every reader keys on to fold a signature away or strip it from a
     * quote. **The trailing space is part of it** — without it the line
     * is two hyphens and nothing recognises it.
     */
    const val SEPARATOR = "-- "

    /**
     * The body as it goes on the wire.
     *
     * An empty signature returns the body untouched rather than a
     * separator with nothing under it. A body that already carries one
     * is left alone: a reply quotes the original beneath what was
     * typed, and a second signature between the two reads as though the
     * sender signed the other person's message.
     */
    fun append(body: String, signature: String): String {
        val sig = signature.trim()
        if (sig.isEmpty()) return body
        if (carriesOne(body)) return body
        val text = body.trim()
        if (text.isEmpty()) return "$SEPARATOR\n$sig"
        return "$text\n\n$SEPARATOR\n$sig"
    }

    /**
     * Whether the text already has a separator line of its own.
     *
     * Split on any line break, not on `"\n"`: a message written by a
     * Windows client is CRLF throughout, and splitting on the bare
     * newline leaves stray carriage returns that stop `"--"` from
     * matching.
     */
    fun carriesOne(body: String): Boolean =
        body.split(Regex("\\r\\n|\\r|\\n")).any { it.trim() == "--" }

    /**
     * Which of an account's signatures to use.
     *
     * The one marked default, and only then the first: a person with
     * two signatures has said which one is theirs, and picking the
     * first would sign work mail "Sent from a phone" for the rest of
     * time.
     */
    fun preferred(signatures: List<Wire.Signature>): String =
        (signatures.firstOrNull { it.isDefault } ?: signatures.firstOrNull())?.textContent.orEmpty()
}
