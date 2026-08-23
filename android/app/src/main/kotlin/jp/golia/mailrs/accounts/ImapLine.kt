package jp.golia.mailrs.accounts

/**
 * Reading what an IMAP server says.
 *
 * Split from the socket so it can be tested without one: every mistake
 * worth making here is in the parsing, and a test that needs a server
 * is a test nobody runs. The same split, and the same cases, as the
 * iOS side — a client that disagrees with itself across platforms is
 * two clients.
 */
object Imap {
    /** One untagged line, as far as this client cares about it. */
    sealed interface Untagged {
        data class ListFolder(val name: String, val attributes: List<String>) : Untagged
        data class Exists(val count: Int) : Untagged
        data class UidValidity(val value: Long) : Untagged
        data class UidNext(val value: Long) : Untagged
        /**
         * What the server says it can do.
         *
         * Announced in two places — the greeting and a `CAPABILITY`
         * reply — and a client that reads only the second asks a
         * question it already has the answer to.
         */
        data class Capabilities(val names: List<String>) : Untagged

        data object Other : Untagged
    }

    /** How a tagged line ended. */
    sealed interface Completion {
        data class Ok(val detail: String) : Completion
        data class No(val detail: String) : Completion
        data class Bad(val detail: String) : Completion
    }

    /**
     * The tagged reply for [tag], or null if this is not one.
     *
     * The tag is matched with a trailing space: `a1` must not match
     * `a10`, and a server is free to interleave `a10`'s reply while
     * `a1` is outstanding.
     */
    fun completion(line: String, tag: String): Completion? {
        if (!line.startsWith("$tag ")) return null
        val rest = line.removePrefix("$tag ").trim()
        val word = rest.substringBefore(' ', rest)
        val detail = rest.removePrefix(word).trim()
        return when (word.uppercase()) {
            "OK" -> Completion.Ok(detail)
            "NO" -> Completion.No(detail)
            "BAD" -> Completion.Bad(detail)
            else -> null
        }
    }

    /** What an untagged line says, as far as this client uses it. */
    fun untagged(line: String): Untagged? {
        if (!line.startsWith("* ")) return null
        val body = line.removePrefix("* ").trim()

        if (body.uppercase().startsWith("LIST ")) return parseList(body)
        if (body.uppercase().startsWith("CAPABILITY")) {
            return Untagged.Capabilities(
                body.split(" ").drop(1).filter { it.isNotEmpty() },
            )
        }
        // Also announced inside the greeting's response code, which is
        // where a server that offers no `CAPABILITY` at all says it.
        val open = body.indexOf("[CAPABILITY ", ignoreCase = true)
        if (open >= 0) {
            val close = body.indexOf(']', open)
            if (close > open) {
                return Untagged.Capabilities(
                    body.substring(open + "[CAPABILITY ".length, close)
                        .split(" ").filter { it.isNotEmpty() },
                )
            }
        }

        val head = body.substringBefore(' ')
        val tail = body.removePrefix(head).trim()
        val n = head.toIntOrNull()
        if (n != null && tail.uppercase().startsWith("EXISTS")) return Untagged.Exists(n)

        bracketed(body, "UIDVALIDITY")?.let { return Untagged.UidValidity(it) }
        bracketed(body, "UIDNEXT")?.let { return Untagged.UidNext(it) }
        return Untagged.Other
    }

    /**
     * `OK [UIDVALIDITY 1234] Ready` -> 1234.
     *
     * The value is inside brackets in a response code, and the text
     * after it is free-form: a server may say anything at all,
     * including something that looks like another number.
     */
    private fun bracketed(body: String, key: String): Long? {
        val open = body.indexOf("[$key ", ignoreCase = true)
        if (open < 0) return null
        val from = open + key.length + 2
        val close = body.indexOf(']', from)
        if (close < 0) return null
        return body.substring(from, close).trim().toLongOrNull()
    }

    /**
     * `LIST (\HasNoChildren \Sent) "/" "[Gmail]/Sent Mail"`
     *
     * The name is last and may be quoted, may contain spaces, and may
     * contain the delimiter — which is why it is taken from the end
     * rather than by splitting on spaces.
     */
    private fun parseList(body: String): Untagged? {
        val open = body.indexOf('(')
        val close = body.indexOf(')')
        if (open < 0 || close < open) return null
        val attributes = body.substring(open + 1, close).split(' ').filter { it.isNotEmpty() }
        val name = lastQuotedOrWord(body.substring(close + 1).trim()) ?: return null
        return Untagged.ListFolder(name, attributes)
    }

    /**
     * The last field of a line, unquoted.
     *
     * Scanned **forwards** tracking escapes: walking back from the end
     * and stopping at the first unescaped quote finds the wrong one
     * when the name itself ends in `\"`.
     */
    fun lastQuotedOrWord(s: String): String? {
        val t = s.trim()
        if (t.isEmpty()) return null
        if (!t.endsWith('"')) return t.split(' ').lastOrNull()?.ifEmpty { null }
        val opens = mutableListOf<Int>()
        var i = 0
        while (i < t.length) {
            when {
                t[i] == '\\' -> i++
                t[i] == '"' -> opens += i
            }
            i++
        }
        if (opens.size < 2) return null
        return t.substring(opens[opens.size - 2] + 1, opens[opens.size - 1])
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    }

    /**
     * What a `FETCH` line announced, when it announced a literal.
     *
     * `* 12 FETCH (UID 4390 FLAGS (\Seen) BODY[] {2048}` — the braces
     * give the byte count, and **the bytes that follow are read by
     * that count rather than scanned for a terminator**. A message
     * body contains every byte sequence a terminator could be made of,
     * so scanning truncates mail at whatever looks like the end.
     */
    data class Announced(val uid: Long?, val seen: Boolean, val literalBytes: Int?)

    /** Read a `FETCH` line. */
    fun fetchLine(line: String): Announced? {
        if (!line.startsWith("* ") || !line.uppercase().contains(" FETCH ")) return null
        val uid = numberAfter("UID ", line)
        // `\Seen` inside the FLAGS list. Matched with the backslash so
        // a folder called "Seen" in the same line cannot set it.
        val seen = line.uppercase().contains("\\SEEN")
        val open = line.lastIndexOf('{')
        val close = line.lastIndexOf('}')
        val literal = if (open in 0..<close) {
            line.substring(open + 1, close).toIntOrNull()
        } else {
            null
        }
        return Announced(uid, seen, literal)
    }

    /** The number after a keyword, or null. */
    private fun numberAfter(keyword: String, line: String): Long? {
        val at = line.indexOf(keyword, ignoreCase = true)
        if (at < 0) return null
        val digits = line.substring(at + keyword.length).takeWhile { it.isDigit() }
        return digits.toLongOrNull()
    }

    /**
     * Quote a mailbox name or a password for the wire.
     *
     * Generated app passwords contain `"` and `\` often enough that an
     * unquoted argument turns one into a syntax error — and the person
     * is told their password is wrong when it is right.
     */
    fun quoted(s: String): String =
        "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\""

    /**
     * Whether a refusal means the credential is wrong rather than the
     * server being unhappy about something else.
     *
     * One is a button to press, the other is waiting.
     */
    fun isAuthenticationFailure(detail: String): Boolean {
        val d = detail.uppercase()
        return d.contains("AUTHENTICATIONFAILED") ||
            d.contains("INVALID CREDENTIALS") ||
            d.contains("LOGIN FAILED") ||
            d.contains("AUTHORIZATIONFAILED")
    }
}
