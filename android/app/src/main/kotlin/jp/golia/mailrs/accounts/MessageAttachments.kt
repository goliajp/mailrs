package jp.golia.mailrs.accounts

/**
 * What is attached to a message.
 *
 * A different question from "what should be shown", which is
 * [MessageBody]'s, and the two genuinely disagree: the part a reader
 * sees is not attached, and a PDF nobody can render still has to be
 * listed. Both walk the same tree through the same primitives, and
 * each applies its own policy to it.
 */
object MessageAttachments {
    /**
     * One attached file, **not yet decoded**.
     *
     * It points into the message rather than carrying a copy: opening
     * a 25 MB message used to hold the raw bytes and about 18 MB of
     * decoded attachments at the same time, for a screen that shows a
     * name and a size. The decoding happens when somebody taps one,
     * and what is decoded is that one.
     *
     * [size] is computed from the encoded length rather than by
     * decoding — base64 is four characters for every three bytes, so
     * the answer is arithmetic, and a size shown before a tap must not
     * cost what the tap costs.
     */
    class Attachment(
        val filename: String,
        val mimeType: String,
        /** The whole message. Every attachment of it shares this. */
        private val source: ByteArray,
        /** Where in [source] the encoded part is. */
        private val from: Int,
        private val until: Int,
        private val transfer: String,
        /**
         * Whether the message meant it to appear inside the text — a
         * signature image, usually. Listed anyway, because a reader
         * shown text has no other way to reach it, but marked so the
         * list can say which is which.
         */
        val inline: Boolean,
    ) {
        /**
         * How big the file is once decoded, without decoding it.
         *
         * base64 carries three bytes in every four characters, and the
         * padding says how many of the last three are real. Line
         * breaks are not characters of the encoding, so they are not
         * counted.
         */
        val size: Int by lazy {
            when (transfer) {
                "base64" -> {
                    var characters = 0
                    var padding = 0
                    for (i in from until until) {
                        val c = source[i].toInt().toChar()
                        when {
                            c == '=' -> padding++
                            c.isLetterOrDigit() || c == '+' || c == '/' -> characters++
                        }
                    }
                    maxOf(0, (characters + padding) / 4 * 3 - padding)
                }
                else -> until - from
            }
        }

        /** The file itself, decoded now. */
        fun decoded(): ByteArray =
            MessageBody.decodeTransfer(source.copyOfRange(from, until), transfer)

        // Compared by what it *is*, not by where it points: two reads
        // of the same message are the same attachment, and an identity
        // comparison on the backing array would say otherwise.
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is Attachment) return false
            return filename == other.filename && mimeType == other.mimeType &&
                inline == other.inline && decoded().contentEquals(other.decoded())
        }

        override fun hashCode(): Int =
            filename.hashCode() * 31 + mimeType.hashCode() * 31 + size
    }

    /** Everything attached, in the order the message lists it. */
    fun of(raw: ByteArray): List<Attachment> {
        val out = mutableListOf<Attachment>()
        walk(raw, out)
        return out
    }

    private fun walk(raw: ByteArray, out: MutableList<Attachment>) {
        val (headerBytes, body) = MessageBody.split(raw)
        val header = String(headerBytes, Charsets.UTF_8)
        val type = MessageBody.contentType(header)
        if (type.type == "multipart") {
            val boundary = type.params["boundary"] ?: return
            if (boundary.isEmpty()) return
            for (piece in MessageBody.pieces(body, boundary)) walk(piece, out)
            return
        }
        val disposition = value("content-disposition", header).orEmpty()
        val kind = disposition.substringBefore(';').trim().lowercase()
        val filename = filename(header)

        // A part is attached when the message says so, or when it is
        // not something a reader could have been shown. A text part
        // with no filename is the message itself.
        val attached = kind == "attachment" || filename != null || type.type != "text"
        if (!attached) return
        out.add(
            Attachment(
                filename = filename ?: fallbackName(type),
                mimeType = "${type.type}/${type.subtype}".trim('/'),
                source = body,
                from = 0,
                until = body.size,
                transfer = MessageBody.encoding(header),
                inline = kind == "inline",
            ),
        )
    }

    /**
     * The name, from wherever it was put.
     *
     * `Content-Disposition: attachment; filename=` is the right place;
     * `Content-Type: ...; name=` is the older one and still arrives.
     * Both may be RFC 2231-encoded, which is how a Japanese filename
     * survives a header that must be ASCII — and a client that does
     * not decode it shows the person `%E6%97%A5%E6%9C%AC.pdf`.
     */
    internal fun filename(header: String): String? {
        val disposition = value("content-disposition", header).orEmpty()
        val type = value("content-type", header).orEmpty()
        for (source in listOf(disposition, type)) {
            rfc2231(source, "filename")?.let { return it }
            rfc2231(source, "name")?.let { return it }
        }
        return null
    }

    /**
     * `filename="x"`, `filename*=utf-8''%E2%80%A6`, and the numbered
     * continuations a long name is split into.
     */
    internal fun rfc2231(source: String, key: String): String? {
        val fields = source.split(';').map { it.trim() }
        // The continuations first: a name split across `key*0*=` and
        // `key*1*=` is not found by looking for `key*=` at all.
        val parts = fields.mapNotNull { field ->
            val eq = field.indexOf('=')
            if (eq < 0) return@mapNotNull null
            val name = field.substring(0, eq).trim().lowercase()
            val match = Regex("^${Regex.escape(key)}\\*(\\d+)(\\*?)$").find(name)
                ?: return@mapNotNull null
            Triple(match.groupValues[1].toInt(), match.groupValues[2] == "*", unquote(field.substring(eq + 1)))
        }.sortedBy { it.first }
        if (parts.isNotEmpty()) {
            val joined = parts.joinToString("") { (_, encoded, value) ->
                when {
                    encoded -> percentDecoded(stripCharset(value))
                    else -> value
                }
            }
            return joined.ifEmpty { null }
        }
        for (field in fields) {
            val eq = field.indexOf('=')
            if (eq < 0) continue
            val name = field.substring(0, eq).trim().lowercase()
            val value = unquote(field.substring(eq + 1))
            if (name == "$key*") return percentDecoded(stripCharset(value))
            if (name == key) return value
        }
        return null
    }

    /** `utf-8''name` -> `name`, keeping the percent-escapes. */
    private fun stripCharset(value: String): String {
        val first = value.indexOf('\'')
        if (first < 0) return value
        val second = value.indexOf('\'', first + 1)
        if (second < 0) return value
        return value.substring(second + 1)
    }

    /** Percent-escapes back to bytes, then to text as UTF-8. */
    private fun percentDecoded(value: String): String {
        val out = java.io.ByteArrayOutputStream(value.length)
        var i = 0
        while (i < value.length) {
            val c = value[i]
            if (c == '%' && i + 2 < value.length) {
                val hex = value.substring(i + 1, i + 3).toIntOrNull(16)
                if (hex != null) {
                    out.write(hex)
                    i += 3
                    continue
                }
            }
            out.write(c.code)
            i++
        }
        return String(out.toByteArray(), Charsets.UTF_8)
    }

    private fun unquote(value: String): String {
        val t = value.trim()
        if (t.length >= 2 && t.startsWith("\"") && t.endsWith("\"")) {
            return t.substring(1, t.length - 1)
        }
        return t
    }

    private fun value(name: String, header: String): String? {
        for (line in MessageHeaders.unfolded(header)) {
            val colon = line.indexOf(':')
            if (colon < 0) continue
            if (line.substring(0, colon).trim().lowercase() == name) {
                return line.substring(colon + 1).trim()
            }
        }
        return null
    }

    /**
     * Something to call a nameless part.
     *
     * Not "attachment": a list of four things all called that is a list
     * nobody can pick from. The type is what is actually known.
     */
    private fun fallbackName(type: MessageBody.ContentType): String {
        val extension = when (type.subtype) {
            "jpeg" -> "jpg"
            "plain" -> "txt"
            else -> type.subtype.ifEmpty { "bin" }
        }
        return "${type.type.ifEmpty { "file" }}.$extension"
    }
}
