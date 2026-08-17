package jp.golia.mailrs.wire

/**
 * What another app asked this one to do.
 *
 * Three ways Android asks a mail client to write a message, and they
 * arrive as different intents carrying different things:
 *
 * - **`mailto:`** — a link tapped anywhere on the phone. RFC 6068: the
 *   path is the recipient, and `?subject=` / `?body=` / `?cc=` may
 *   follow. Percent-decoded, because `mailto:a@b?subject=Hello%20there`
 *   means a space and not the characters `%20`.
 * - **Share text** — `EXTRA_TEXT`, usually a URL, with `EXTRA_SUBJECT`
 *   the page's title.
 * - **Share files** — one or many `content://` URIs.
 *
 * Pure, so the parsing can be read and tested without an Activity. The
 * URI handling is the part that goes wrong quietly: a subject that
 * arrives still percent-encoded looks like the sender typed it.
 */
object ShareIntent {

    /** What a `mailto:` URI names. Every field may be absent. */
    data class Mailto(
        val to: String = "",
        val cc: String = "",
        val bcc: String = "",
        val subject: String = "",
        val body: String = "",
    )

    /**
     * Parse a `mailto:` URI.
     *
     * `Uri.parse` is deliberately not used: it treats the whole thing as
     * opaque and its `getQueryParameter` returns null for exactly this
     * shape, which is how a subject silently disappears.
     */
    fun mailto(uri: String): Mailto {
        val rest = uri.removePrefix("mailto:")
        val path = rest.substringBefore('?')
        val query = rest.substringAfter('?', "")
        var out = Mailto(to = decode(path))
        if (query.isEmpty()) return out
        for (pair in query.split('&')) {
            if (pair.isEmpty()) continue
            val name = pair.substringBefore('=').lowercase()
            val value = decode(pair.substringAfter('=', ""))
            out = when (name) {
                // `to` in the query adds to the path's recipients rather
                // than replacing them — RFC 6068 allows both, and a
                // client that overwrote would drop the addressee the
                // link led with.
                "to" -> out.copy(to = listOf(out.to, value).filter(String::isNotEmpty).joinToString(", "))
                "cc" -> out.copy(cc = value)
                "bcc" -> out.copy(bcc = value)
                "subject" -> out.copy(subject = value)
                "body" -> out.copy(body = value)
                else -> out
            }
        }
        return out
    }

    /**
     * `+` is a space only in form encoding, and a `mailto:` query is not
     * a form — an address like `a+tag@example.com` is common and must
     * survive.
     */
    private fun decode(s: String): String =
        runCatching { java.net.URLDecoder.decode(s.replace("+", "%2B"), "UTF-8") }.getOrDefault(s)
}
