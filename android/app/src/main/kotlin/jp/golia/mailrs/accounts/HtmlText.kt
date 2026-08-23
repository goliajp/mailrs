package jp.golia.mailrs.accounts

/**
 * Readable text out of an HTML mail body.
 *
 * Not a renderer, and deliberately not a WebView. A mail body's markup
 * arrives with remote images in it, and every one of those is a request
 * to somebody else's server the moment a message is opened — it reports
 * that the mail was read, when, and from what address. Mail clients call
 * the setting "load remote content" and ship it off. Extracting the text
 * does not have the setting at all.
 */
object HtmlText {
    /** Blocks that end a line when they close. */
    private val BLOCKS = setOf(
        "p", "div", "br", "tr", "li", "h1", "h2", "h3", "h4", "h5", "h6",
        "blockquote", "table", "ul", "ol", "section", "article", "pre",
    )

    /** Elements whose contents are not text at all. */
    private val SILENT = setOf("script", "style", "head", "title")

    fun plain(html: String): String {
        val out = StringBuilder()
        val tag = StringBuilder()
        var inTag = false
        var skipUntil: String? = null

        for ((i, ch) in html.withIndex()) {
            when {
                // `<` opens a tag only when what follows could name one.
                // Mail arrives with `a < b`, and with plain text
                // mislabelled as HTML; treating every `<` as a tag eats
                // the rest of the line in both cases.
                ch == '<' && startsATag(html, i) -> {
                    inTag = true
                    tag.clear()
                }
                ch == '>' && inTag -> {
                    inTag = false
                    val raw = tag.toString()
                    val name = tagName(raw)
                    val closing = raw.startsWith("/")
                    when {
                        skipUntil != null -> if (closing && name == skipUntil) skipUntil = null
                        // A self-closed `<style/>` never gets its closing
                        // tag, and waiting for one swallows the message.
                        name in SILENT && !closing && !raw.endsWith("/") -> skipUntil = name
                        name in SILENT -> Unit
                        name in BLOCKS -> if (!out.endsWith("\n")) out.append('\n')
                    }
                }
                inTag -> tag.append(ch)
                skipUntil != null -> Unit
                else -> out.append(ch)
            }
        }
        return tidy(entities(out.toString()))
    }

    private fun startsATag(html: String, i: Int): Boolean {
        val next = html.getOrNull(i + 1) ?: return false
        // `!` for comments and the doctype, which are dropped whole.
        return next.isLetter() || next == '/' || next == '!'
    }

    private fun tagName(tag: String): String =
        tag.removePrefix("/").takeWhile { !it.isWhitespace() && it != '/' }.lowercase()

    /**
     * The handful that actually appear in mail, plus numeric ones.
     *
     * `&nbsp;` becomes an ordinary space rather than U+00A0: a
     * non-breaking space is invisible and unbreakable, and a paragraph
     * full of them will not wrap on a phone.
     */
    private fun entities(s: String): String {
        val out = StringBuilder(s.length)
        var i = 0
        while (i < s.length) {
            if (s[i] != '&') {
                out.append(s[i])
                i++
                continue
            }
            val semi = s.indexOf(';', i)
            if (semi < 0 || semi - i > 10) {
                out.append(s[i])
                i++
                continue
            }
            val name = s.substring(i + 1, semi)
            val decoded = decode(name)
            if (decoded == null) {
                out.append(s, i, semi + 1)
            } else {
                out.append(decoded)
            }
            i = semi + 1
        }
        return out.toString()
    }

    private fun decode(name: String): String? {
        when (name.lowercase()) {
            "amp" -> return "&"
            "lt" -> return "<"
            "gt" -> return ">"
            "quot" -> return "\""
            "apos", "#39" -> return "'"
            "nbsp" -> return " "
            "mdash" -> return "—"
            "ndash" -> return "–"
            "hellip" -> return "…"
            "rsquo" -> return "’"
            "lsquo" -> return "‘"
            "ldquo" -> return "“"
            "rdquo" -> return "”"
        }
        if (!name.startsWith("#")) return null
        val digits = name.drop(1)
        val value = when {
            digits.startsWith("x") || digits.startsWith("X") ->
                digits.drop(1).toIntOrNull(16)
            else -> digits.toIntOrNull()
        } ?: return null
        if (value < 0 || value > 0x10FFFF) return null
        return String(Character.toChars(value))
    }

    /**
     * Collapse the whitespace markup leaves behind.
     *
     * Mail HTML is generated, and generated markup is indented: every
     * newline and run of spaces between tags is layout, not text.
     *
     * **Blank lines go entirely.** Every block already ends its own
     * line, so paragraphs stay apart without them, and what is left when
     * they are kept is one blank line per level of indentation in
     * somebody's template — which is most of the screen on marketing
     * mail.
     */
    private fun tidy(s: String): String =
        // A lone CR is a line ending too — old Mac files, and the tail
        // of a message whose last line was never terminated. Kotlin's
        // `trim()` would remove it anyway, but the split must see it as
        // a line break or two lines arrive as one.
        s.replace("\r\n", "\n")
            .replace("\r", "\n")
            .split("\n")
            .map { line -> line.split(" ").filter { it.isNotEmpty() }.joinToString(" ").trim() }
            .filter { it.isNotEmpty() }
            .joinToString("\n")
}
