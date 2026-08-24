package jp.golia.mailrs.accounts

import java.nio.charset.Charset
import java.util.Base64

/**
 * RFC 2047 encoded words — `=?UTF-8?B?...?=`.
 *
 * A header may only hold ASCII, so anything else arrives encoded.
 * Without this every Japanese or Chinese subject in the list is a run
 * of `=?UTF-8?B?` gibberish, which is the most visible way a mail
 * client can look broken.
 */
object EncodedWord {
    /**
     * Decode every encoded word in a header value.
     *
     * Text outside the words is left exactly as it is: a subject is
     * often half encoded and half not, and re-encoding the plain half
     * would corrupt it.
     */
    /**
     * An encoded word decodes to **anything at all**, including a
     * CRLF — and a header value cannot contain one. Folding is
     * expressed by the encoding, never by the content, so a line break
     * coming out of here did not come from a header: it came from
     * somebody who wanted one somewhere it does not belong.
     *
     * Left in, it reached `RCPT TO:<…>` when replying (SMTP command
     * injection: the message also went to an address the sender never
     * typed) and the outgoing `To:` and `Subject:` (a `Bcc:` header
     * the sender never wrote). Stripped here, at the boundary, rather
     * than at each of the places that would have to remember.
     */
    private fun withoutControls(text: String): String =
        text.filter { it.code >= 0x20 && it.code != 0x7F }

    fun decode(value: String): String {
        if (!value.contains("=?")) return withoutControls(value)
        val out = StringBuilder()
        var rest = value
        // RFC 2047 6.2: whitespace **between two encoded words** is not
        // part of the text — it is there so the words can be folded,
        // and a decoder that keeps it puts a space in the middle of
        // every long CJK subject.
        var previousWasWord = false

        while (true) {
            val start = rest.indexOf("=?")
            if (start < 0) break
            val before = rest.substring(0, start)
            val word = readWord(rest.substring(start))
            if (word == null) {
                out.append(before).append("=?")
                rest = rest.substring(start + 2)
                previousWasWord = false
                continue
            }
            if (!(previousWasWord && before.isNotEmpty() && before.isBlank())) {
                out.append(before)
            }
            out.append(word.text)
            rest = word.rest
            previousWasWord = true
        }
        out.append(rest)
        return withoutControls(out.toString())
    }

    private data class Word(val text: String, val rest: String)

    /** One `=?charset?enc?payload?=`, from a slice that starts at `=?`. */
    /**
     * The other direction: a header value that a receiving client can
     * read.
     *
     * **ASCII passes through untouched.** Encoding a plain subject
     * makes it unreadable in the raw message and gains nothing — which
     * is why this checks rather than always encoding.
     *
     * Base64 rather than quoted-printable, because a subject that needs
     * encoding at all is usually not Latin: a CJK subject in
     * quoted-printable is three `=XX` per character and grows past the
     * line limit almost at once.
     */
    fun encode(text: String): String {
        if (text.all { it.code < 128 }) return text
        // 75 is the RFC 2047 limit for a whole encoded word including
        // its `=?utf-8?B?` and `?=`, and base64 is 4 characters per 3
        // bytes, so each chunk may carry 45 bytes at most.
        return utf8Chunks(text, 45).joinToString("\r\n ") {
            "=?utf-8?B?" + java.util.Base64.getEncoder().encodeToString(it) + "?="
        }
    }

    /**
     * Split into runs of at most [limit] bytes, **never through a
     * character**. Cutting UTF-8 mid-sequence produces an encoded word
     * that decodes to a replacement character on every client.
     */
    private fun utf8Chunks(text: String, limit: Int): List<ByteArray> {
        val out = mutableListOf<ByteArray>()
        var current = java.io.ByteArrayOutputStream()
        // By code point, not by `Char`: a `Char` is half a surrogate
        // pair, and splitting there is the same defect one level down.
        var i = 0
        while (i < text.length) {
            val point = text.codePointAt(i)
            val encoded = String(Character.toChars(point)).toByteArray(Charsets.UTF_8)
            if (current.size() + encoded.size > limit && current.size() > 0) {
                out.add(current.toByteArray())
                current = java.io.ByteArrayOutputStream()
            }
            current.write(encoded)
            i += Character.charCount(point)
        }
        if (current.size() > 0) out.add(current.toByteArray())
        return out
    }

    private fun readWord(s: String): Word? {
        val body = s.substring(2)
        val firstQ = body.indexOf('?')
        if (firstQ < 0) return null
        val secondQ = body.indexOf('?', firstQ + 1)
        if (secondQ < 0) return null
        val close = body.indexOf("?=", secondQ + 1)
        if (close < 0) return null

        val charset = body.substring(0, firstQ).uppercase()
        val encoding = body.substring(firstQ + 1, secondQ).uppercase()
        val payload = body.substring(secondQ + 1, close)

        val bytes = when (encoding) {
            "B" -> runCatching { Base64.getDecoder().decode(padded(payload)) }.getOrNull()
            "Q" -> quotedPrintable(payload)
            else -> null
        } ?: return null

        val text = decodeBytes(bytes, charset) ?: return null
        return Word(text, s.substring(2 + close + 2))
    }

    /**
     * Base64 without its padding is common in the wild and fails to
     * decode without it.
     */
    private fun padded(s: String): String {
        val short = s.length % 4
        return if (short == 0) s else s + "=".repeat(4 - short)
    }

    /** Q-encoding: `_` is a space, `=XX` is a byte. */
    private fun quotedPrintable(s: String): ByteArray? {
        val out = java.io.ByteArrayOutputStream()
        var i = 0
        while (i < s.length) {
            val c = s[i]
            when {
                c == '_' -> {
                    out.write(0x20)
                    i++
                }
                c == '=' && i + 2 < s.length -> {
                    val b = s.substring(i + 1, i + 3).toIntOrNull(16) ?: return null
                    out.write(b)
                    i += 3
                }
                else -> {
                    out.write(c.toString().toByteArray())
                    i++
                }
            }
        }
        return out.toByteArray()
    }

    /**
     * The charsets that actually turn up. An unknown one returns null
     * so the raw word is left visible — mojibake somebody can report
     * beats text this app invented.
     */
    private fun decodeBytes(bytes: ByteArray, charset: String): String? {
        val name = when (charset) {
            "UTF-8", "UTF8" -> "UTF-8"
            "ISO-8859-1", "LATIN1" -> "ISO-8859-1"
            "ISO-2022-JP" -> "ISO-2022-JP"
            "SHIFT_JIS", "SHIFT-JIS", "SJIS" -> "Shift_JIS"
            "EUC-JP" -> "EUC-JP"
            "GB2312", "GBK", "GB18030" -> "GB18030"
            else -> return null
        }
        return runCatching { String(bytes, Charset.forName(name)) }.getOrNull()
    }
}
