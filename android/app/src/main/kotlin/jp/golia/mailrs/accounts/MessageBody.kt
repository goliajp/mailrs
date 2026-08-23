package jp.golia.mailrs.accounts

import java.nio.charset.Charset
import java.util.Base64

/**
 * The readable part of a message, out of its raw bytes.
 *
 * Not a full MIME implementation and not trying to be: what a reader
 * needs is the one part worth showing and the text of it. Attachments,
 * signatures and the rest of the tree are left alone.
 *
 * Bytes rather than a `String` throughout, and that is the whole reason
 * this exists separately from [MessageHeaders]. A message says what its
 * charset is *inside itself*; decoding it as UTF-8 on the way in — which
 * is what reading a socket into a `String` does — turns every Shift_JIS
 * and windows-1252 message into replacement characters before anything
 * has read the header that would have said so.
 */
object MessageBody {
    data class Display(val text: String, val isHtml: Boolean) {
        companion object {
            val EMPTY = Display("", false)
        }
    }

    /** The part worth showing. */
    fun extract(raw: ByteArray): Display {
        val (header, body) = split(raw)
        return part(String(header, Charsets.UTF_8), body)
    }

    private fun part(header: String, body: ByteArray): Display {
        val type = contentType(header)
        if (type.type == "multipart") {
            val boundary = type.params["boundary"]
            if (boundary.isNullOrEmpty()) {
                // A multipart with no boundary cannot be taken apart.
                // Showing the raw source beats showing nothing: the text
                // is usually in there, sandwiched between header blocks
                // a person can read past. Deliberately not via
                // `decoded`, which would throw it away for not being
                // `text/*` — it is not text/* by declaration only.
                return Display(
                    string(decodeTransfer(body, encoding(header)), type.params["charset"]),
                    false,
                )
            }
            return choose(pieces(body, boundary), type.subtype)
        }
        return decoded(header, body)
    }

    /**
     * Which of a multipart's pieces to show.
     *
     * `alternative` is the same message written twice, so the choice is
     * a preference: plain text first, markup only when there is no plain
     * text. Every other kind — `mixed`, `related`, `signed` — is a
     * message plus its attachments, and the first piece with anything
     * readable in it is the message.
     */
    private fun choose(parts: List<ByteArray>, kind: String): Display {
        val shown = parts.map { piece ->
            val (h, b) = split(piece)
            part(String(h, Charsets.UTF_8), b)
        }
        if (kind == "alternative") {
            shown.firstOrNull { !it.isHtml && it.text.isNotEmpty() }?.let { return it }
        }
        return shown.firstOrNull { it.text.isNotEmpty() } ?: Display.EMPTY
    }

    /**
     * The pieces between the boundary delimiters.
     *
     * Everything before the first delimiter is the preamble and
     * everything after the closing one is the epilogue; both are there
     * for mail readers that cannot do MIME at all, and neither is part
     * of the message.
     */
    private fun pieces(body: ByteArray, boundary: String): List<ByteArray> {
        val delimiter = "--$boundary".toByteArray()
        // The delimiter must begin a line, or a boundary string that
        // happens to appear inside a part cuts it in half.
        val starts = indexesOf(delimiter, body).filter { it == 0 || body[it - 1] == '\n'.code.toByte() }
        if (starts.isEmpty()) return emptyList()
        val out = mutableListOf<ByteArray>()
        for ((i, start) in starts.withIndex()) {
            val after = start + delimiter.size
            // The closing delimiter: nothing after it is ours.
            if (after + 1 < body.size && body[after] == '-'.code.toByte() &&
                body[after + 1] == '-'.code.toByte()
            ) {
                break
            }
            if (i + 1 >= starts.size) break
            val from = skipLine(body, after)
            val to = starts[i + 1]
            if (from < to) out.add(body.copyOfRange(from, to))
        }
        return out
    }

    private fun decoded(header: String, body: ByteArray): Display {
        val type = contentType(header)
        // Anything that is not text is not something to show as text. An
        // attached PDF decoded as if it were a message reads as a screen
        // of noise.
        if (type.type.isNotEmpty() && type.type != "text") return Display.EMPTY
        val bytes = decodeTransfer(body, encoding(header))
        return Display(string(bytes, type.params["charset"]), type.subtype == "html")
    }

    private fun decodeTransfer(body: ByteArray, cte: String): ByteArray = when (cte) {
        "base64" -> Base64Body.decode(body)
        "quoted-printable" -> QuotedPrintable.decode(body)
        else -> body
    }

    /**
     * Bytes to text, in the charset the message declared.
     *
     * UTF-8 when nothing was declared — it is what most mail is, and it
     * is self-checking, so a wrong guess fails loudly enough to fall
     * back rather than producing plausible nonsense.
     */
    private fun string(bytes: ByteArray, charset: String?): String {
        if (charset.isNullOrEmpty()) return String(bytes, Charsets.UTF_8)
        return try {
            String(bytes, Charset.forName(charset))
        } catch (_: Exception) {
            String(bytes, Charsets.UTF_8)
        }
    }

    data class ContentType(
        val type: String = "",
        val subtype: String = "",
        val params: Map<String, String> = emptyMap(),
    )

    fun contentType(header: String): ContentType {
        val raw = valueOf("content-type", header) ?: return ContentType("text", "plain")
        val fields = splitOnSemicolons(raw)
        val full = fields.firstOrNull().orEmpty()
        val halves = full.split("/", limit = 2)
        val params = mutableMapOf<String, String>()
        for (field in fields.drop(1)) {
            val eq = field.indexOf('=')
            if (eq < 0) continue
            val name = field.substring(0, eq).trim().lowercase()
            var v = field.substring(eq + 1).trim()
            if (v.length >= 2 && v.startsWith("\"") && v.endsWith("\"")) v = v.substring(1, v.length - 1)
            params[name] = v
        }
        return ContentType(
            halves.getOrNull(0)?.trim()?.lowercase().orEmpty(),
            halves.getOrNull(1)?.trim()?.lowercase().orEmpty(),
            params,
        )
    }

    private fun encoding(header: String): String =
        valueOf("content-transfer-encoding", header)?.trim()?.lowercase().orEmpty()

    private fun valueOf(name: String, header: String): String? {
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
     * Split on the semicolons that separate parameters, ignoring the
     * ones inside a quoted value — a boundary may contain one, and
     * splitting there loses the rest of it.
     */
    private fun splitOnSemicolons(s: String): List<String> {
        val out = mutableListOf<String>()
        val current = StringBuilder()
        var quoted = false
        for (ch in s) {
            if (ch == '"') quoted = !quoted
            if (ch == ';' && !quoted) {
                out.add(current.toString().trim())
                current.clear()
                continue
            }
            current.append(ch)
        }
        out.add(current.toString().trim())
        return out.filter { it.isNotEmpty() }
    }

    /** Header block and body, at the first blank line. */
    fun split(raw: ByteArray): Pair<ByteArray, ByteArray> {
        indexesOf("\r\n\r\n".toByteArray(), raw).firstOrNull()?.let {
            return raw.copyOfRange(0, it) to raw.copyOfRange(it + 4, raw.size)
        }
        indexesOf("\n\n".toByteArray(), raw).firstOrNull()?.let {
            return raw.copyOfRange(0, it) to raw.copyOfRange(it + 2, raw.size)
        }
        return raw to ByteArray(0)
    }

    private fun skipLine(d: ByteArray, from: Int): Int {
        var i = from
        while (i < d.size && d[i] != '\n'.code.toByte()) i++
        return minOf(i + 1, d.size)
    }

    private fun indexesOf(needle: ByteArray, haystack: ByteArray): List<Int> {
        if (needle.isEmpty() || haystack.size < needle.size) return emptyList()
        val out = mutableListOf<Int>()
        for (i in 0..haystack.size - needle.size) {
            var match = true
            for (k in needle.indices) {
                if (haystack[i + k] != needle[k]) {
                    match = false
                    break
                }
            }
            if (match) out.add(i)
        }
        return out
    }
}

/** RFC 2045 §6.7. */
object QuotedPrintable {
    fun decode(input: ByteArray): ByteArray {
        val out = java.io.ByteArrayOutputStream(input.size)
        var i = 0
        while (i < input.size) {
            val byte = input[i]
            if (byte != '='.code.toByte()) {
                out.write(byte.toInt())
                i++
                continue
            }
            // A soft line break: `=` at end of line means the line was
            // wrapped there and there is no character at all.
            if (i + 2 < input.size && input[i + 1] == '\r'.code.toByte() &&
                input[i + 2] == '\n'.code.toByte()
            ) {
                i += 3
                continue
            }
            if (i + 1 < input.size && input[i + 1] == '\n'.code.toByte()) {
                i += 2
                continue
            }
            val hi = if (i + 2 < input.size) hex(input[i + 1]) else null
            val lo = if (i + 2 < input.size) hex(input[i + 2]) else null
            if (hi != null && lo != null) {
                out.write(hi shl 4 or lo)
                i += 3
                continue
            }
            // A lone `=` is not valid, and the choice matters: dropping
            // it silently loses a character somebody typed, so it is
            // kept as itself.
            out.write(byte.toInt())
            i++
        }
        return out.toByteArray()
    }

    private fun hex(b: Byte): Int? = when (val c = b.toInt().toChar()) {
        in '0'..'9' -> c - '0'
        in 'A'..'F' -> c - 'A' + 10
        in 'a'..'f' -> c - 'a' + 10
        else -> null
    }
}

/**
 * Base64 as it arrives in mail: wrapped across lines, and sometimes with
 * characters no encoder should have emitted.
 */
object Base64Body {
    fun decode(input: ByteArray): ByteArray {
        val cleaned = input.filter { b ->
            val c = b.toInt().toChar()
            c in 'A'..'Z' || c in 'a'..'z' || c in '0'..'9' || c == '+' || c == '/' || c == '='
        }.toByteArray()
        var text = String(cleaned, Charsets.US_ASCII).trimEnd('=')
        // Padding is often missing from the last line. Without this a
        // whole message decodes to nothing rather than to itself.
        when (text.length % 4) {
            2 -> text += "=="
            3 -> text += "="
            1 -> text = text.dropLast(1)
        }
        return try {
            Base64.getDecoder().decode(text)
        } catch (_: IllegalArgumentException) {
            ByteArray(0)
        }
    }
}
