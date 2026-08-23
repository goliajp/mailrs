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
    fun decode(value: String): String {
        if (!value.contains("=?")) return value
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
        return out.toString()
    }

    private data class Word(val text: String, val rest: String)

    /** One `=?charset?enc?payload?=`, from a slice that starts at `=?`. */
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
