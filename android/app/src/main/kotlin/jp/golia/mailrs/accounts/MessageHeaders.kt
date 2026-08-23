package jp.golia.mailrs.accounts

/**
 * The few headers a list row needs, out of a raw message.
 *
 * Not a MIME parser: this reads what a row shows — who it is from,
 * what it is about, when it arrived, and the identity that threads it.
 * The body is somebody else's problem.
 */
object MessageHeaders {
    data class Parsed(
        val messageId: String = "",
        val from: String = "",
        val subject: String = "",
        val date: String = "",
        val inReplyTo: String = "",
    )

    /**
     * Read the header block.
     *
     * Stops at the blank line that ends it — a body may contain
     * anything, including lines that look exactly like headers, and a
     * parser that keeps going reads them.
     */
    fun parse(raw: String): Parsed {
        var out = Parsed()
        for (line in unfolded(raw)) {
            val colon = line.indexOf(':')
            if (colon <= 0) continue
            val name = line.substring(0, colon).lowercase()
            val value = line.substring(colon + 1).trim()
            out = when (name) {
                "message-id" -> if (out.messageId.isEmpty()) out.copy(messageId = value) else out
                "from" -> if (out.from.isEmpty()) out.copy(from = EncodedWord.decode(value)) else out
                // Decoded here rather than at the call site: every reader
                // of a subject wants the text, and one that forgets
                // shows `=?UTF-8?B?` to somebody.
                "subject" -> if (out.subject.isEmpty()) {
                    out.copy(subject = EncodedWord.decode(value))
                } else {
                    out
                }
                "date" -> if (out.date.isEmpty()) out.copy(date = value) else out
                "in-reply-to" -> if (out.inReplyTo.isEmpty()) out.copy(inReplyTo = value) else out
                else -> out
            }
        }
        return out
    }

    /**
     * The header block, one logical header per element.
     *
     * **Folding is the trap.** RFC 5322 lets a header continue on the
     * next line when it starts with a space or a tab, and a long
     * Subject usually does. A parser that reads lines rather than
     * headers gets half a subject — and, worse, may read the
     * continuation as a header of its own.
     */
    fun unfolded(raw: String): List<String> {
        val out = mutableListOf<String>()
        for (line in raw.replace("\r\n", "\n").split("\n")) {
            if (line.isEmpty()) break // the blank line ends the block
            if ((line.startsWith(" ") || line.startsWith("\t")) && out.isNotEmpty()) {
                out[out.size - 1] = out.last() + " " + line.trim()
            } else {
                out += line
            }
        }
        return out
    }

    /**
     * The display name from a `From`, or the address.
     *
     * `Alice Smith <alice@example.com>` becomes `Alice Smith`; a bare
     * address is its own name. Quotes come off, because a name with a
     * comma in it is quoted and the quotes are syntax.
     */
    fun senderName(from: String): String {
        val t = from.trim()
        val open = t.lastIndexOf('<')
        if (open < 0) return t
        val name = t.substring(0, open).trim()
        if (name.isEmpty()) {
            val close = t.lastIndexOf('>')
            return if (close > open) t.substring(open + 1, close) else t
        }
        if (name.length >= 2 && name.startsWith('"') && name.endsWith('"')) {
            return name.substring(1, name.length - 1).replace("\\\"", "\"")
        }
        return name
    }
}
