package jp.golia.mailrs.wire

/**
 * The composer's markdown as the HTML a send puts in `html_body`, the way the
 * web's send path renders it (`renderMarkdownForSend` in
 * `web/src/lib/render-markdown.ts`), held to the same bytes by the shared
 * `markdown-samples.json`; iOS has the same in `Wire/MarkdownHtml.swift`.
 *
 * The subset a phone composer writes or a reply carries: paragraphs and line
 * breaks, headings, quotes, simple lists, bold, italic, code, links and bare
 * URLs and addresses. Other markdown (tables, fenced code, nested lists) stays
 * text. What a person types is never markup: `<` and `&` arrive escaped, and a
 * link is a link only for http, https and mailto.
 */
object MarkdownHtml {
    /**
     * `html_body` for a body, or null when it has no bold, italic or link of
     * its own — then the message goes as text alone, as it always has.
     */
    fun forSend(body: String, wrapped: Boolean = true): String? {
        if (!hasMarks(body)) return null
        val html = render(body)
        if (wrapped) return wrap(html)
        return html
    }

    fun hasMarks(body: String): Boolean {
        val probe = inline(body, autolinks = false)
        return probe.contains("<strong>") || probe.contains("<em>") || probe.contains("<a ")
    }

    /** The whole body, blank lines kept as empty paragraphs. */
    fun render(markdown: String): String = blocks(preserveBlankLines(markdown.replace("\r\n", "\n")))

    fun wrap(body: String): String = TEMPLATE_HEAD + body + TEMPLATE_TAIL

    private sealed interface Open {
        data object None : Open
        data class Paragraph(val lines: List<String>) : Open
        data class Quote(val lines: List<String>) : Open
        data class ListBlock(val ordered: Boolean, val start: Int, val marker: String, val items: List<String>) : Open
    }

    private class Item(val ordered: Boolean, val start: Int, val marker: String, val text: String)

    private fun blocks(text: String): String {
        val out = StringBuilder()
        var open: Open = Open.None
        fun close() {
            out.append(finish(open))
            open = Open.None
        }
        for (line in text.split("\n")) {
            if (line.isBlank()) {
                close()
                continue
            }
            val heading = heading(line)
            if (heading != null) {
                close()
                out.append(heading)
                continue
            }
            val quoted = quoteLine(line)
            if (quoted != null) {
                val current = open
                if (current is Open.Quote) {
                    open = Open.Quote(current.lines + quoted)
                } else {
                    close()
                    open = Open.Quote(listOf(quoted))
                }
                continue
            }
            val item = listItem(line)
            if (item != null) {
                val current = open
                if (current is Open.ListBlock && current.marker == item.marker) {
                    open = current.copy(items = current.items + item.text)
                    continue
                }
                if (current is Open.Paragraph && item.ordered && item.start != 1) {
                    open = appendLine(current, line)
                    continue
                }
                close()
                open = Open.ListBlock(item.ordered, item.start, item.marker, listOf(item.text))
                continue
            }
            open = appendLine(open, line)
        }
        close()
        return out.toString()
    }

    // a line that starts nothing: the paragraph, quote or last item it continues
    private fun appendLine(open: Open, line: String): Open = when (open) {
        Open.None -> Open.Paragraph(listOf(line))
        is Open.Paragraph -> Open.Paragraph(open.lines + line)
        is Open.Quote -> Open.Quote(open.lines + line)
        is Open.ListBlock -> open.copy(items = open.items.dropLast(1) + (open.items.last() + "\n" + line))
    }

    private fun finish(open: Open): String = when (open) {
        Open.None -> ""
        is Open.Paragraph -> "<p>" + breaks(inline(open.lines.joinToString("\n"))) + "</p>\n"
        is Open.Quote -> "<blockquote>\n" + blocks(open.lines.joinToString("\n")) + "</blockquote>\n"
        is Open.ListBlock -> {
            val tag = when (open.ordered) {
                true -> "ol"
                false -> "ul"
            }
            val start = when (open.ordered && open.start != 1) {
                true -> " start=\"${open.start}\""
                false -> ""
            }
            val body = open.items.joinToString("") { "<li>" + breaks(inline(it)) + "</li>\n" }
            "<$tag$start>\n$body</$tag>\n"
        }
    }

    private fun heading(line: String): String? {
        val m = HEADING.find(line) ?: return null
        val level = m.groupValues[1].length
        return "<h$level>" + inline(m.groupValues[2]) + "</h$level>\n"
    }

    private fun quoteLine(line: String): String? = QUOTE.find(line)?.groupValues?.get(1)

    private fun listItem(line: String): Item? {
        BULLET.find(line)?.let { return Item(false, 1, it.groupValues[1], it.groupValues[2]) }
        NUMBERED.find(line)?.let {
            return Item(true, it.groupValues[1].toIntOrNull() ?: 1, it.groupValues[2], it.groupValues[3])
        }
        return null
    }

    private fun breaks(html: String): String = html.replace("\n", "<br>")

    private fun preserveBlankLines(md: String): String =
        BLANK_RUN.replace(md) { "\n\n" + "&nbsp;\n\n".repeat(it.value.length - 1) }

    /**
     * One run of text: links, code and escapes first, held aside as
     * placeholders so the emphasis rules cannot reach into them.
     */
    fun inline(text: String, autolinks: Boolean = true): String {
        val held = mutableListOf<String>()
        fun hold(html: String): String {
            held += html
            return "${held.size - 1}"
        }
        var s = BACKSLASH.replace(text) { hold(escape(it.groupValues[1])) }
        s = CODE.replace(s) { hold("<code>" + escape(it.groupValues[1]) + "</code>") }
        s = LINK.replace(s) { m ->
            val label = inline(m.groupValues[1], autolinks = false)
            val href = m.groupValues[2]
            if (!isSafe(href)) return@replace hold(label)
            hold("<a href=\"" + escape(encodeURI(href)) + "\">" + label + "</a>")
        }
        if (autolinks) {
            s = URL.replace(s) { m ->
                val (url, rest) = trimUrl(m.groupValues[1])
                val href = when (url.startsWith("www.")) {
                    true -> "http://$url"
                    false -> url
                }
                hold("<a href=\"" + escape(encodeURI(href)) + "\">" + escape(url) + "</a>") + rest
            }
            s = EMAIL.replace(s) { m ->
                val (address, rest) = trimAddress(m.groupValues[1])
                hold("<a href=\"mailto:" + escape(encodeURI(address)) + "\">" + escape(address) + "</a>") + rest
            }
        }
        s = escape(s)
        s = BOLD_STAR.replace(s) { "<strong>" + it.groupValues[1] + "</strong>" }
        s = BOLD_UNDER.replace(s) { "<strong>" + it.groupValues[1] + "</strong>" }
        s = EM_STAR.replace(s) { "<em>" + it.groupValues[1] + "</em>" }
        s = EM_UNDER.replace(s) { "<em>" + it.groupValues[1] + "</em>" }
        // held pieces are finished HTML (a link's label was rendered whole), so one pass puts every one back
        return HELD.replace(s) { held[it.groupValues[1].toInt()] }
    }

    private fun isSafe(href: String): Boolean {
        val lower = href.lowercase()
        return lower.startsWith("http:") || lower.startsWith("https:") || lower.startsWith("mailto:")
    }

    // GFM drops trailing punctuation from a bare URL, and a closing paren it did not open
    private fun trimUrl(raw: String): Pair<String, String> {
        var url = raw
        var rest = ""
        while (url.isNotEmpty()) {
            val last = url.last()
            val unbalanced = last == ')' && url.count { it == ')' } > url.count { it == '(' }
            if (last !in "?!.,:*_~'\"" && !unbalanced) break
            rest = last + rest
            url = url.dropLast(1)
        }
        return url to rest
    }

    private fun trimAddress(raw: String): Pair<String, String> {
        var address = raw
        var rest = ""
        while (address.isNotEmpty() && address.last() in ".-_") {
            rest = address.last() + rest
            address = address.dropLast(1)
        }
        return address to rest
    }

    /** `&` (unless it starts an entity), `<`, `>`, `"`, `'` — what marked escapes. */
    fun escape(text: String): String = BARE_AMP.replace(text, "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&#39;")

    /** JavaScript's `encodeURI`, keeping `%` as marked does. */
    fun encodeURI(href: String): String {
        val out = StringBuilder()
        for (byte in href.toByteArray(Charsets.UTF_8)) {
            val c = (byte.toInt() and 0xff).toChar()
            if (c.code < 128 && (c.isLetterOrDigit() || c in URI_SAFE)) {
                out.append(c)
            } else {
                out.append('%').append("%02X".format(byte.toInt() and 0xff))
            }
        }
        return out.toString()
    }

    private const val URI_SAFE = ";,/?:@&=+$-_.!~*'()#%"
    private val UNICODE = "(?U)"
    private val HEADING = Regex("^ {0,3}(#{1,6})(?:[ \\t]+(.*?))?[ \\t]*$")
    private val QUOTE = Regex("^ {0,3}> ?(.*)$")
    private val BULLET = Regex("^ {0,3}([-*+])[ \\t]+(.*)$")
    private val NUMBERED = Regex("^ {0,3}(\\d{1,9})([.)])[ \\t]+(.*)$")
    private val BLANK_RUN = Regex("\\n{2,}")
    private val BACKSLASH = Regex("\\\\([!-/:-@\\[-`{-~])")
    private val CODE = Regex("`([^`]+)`")
    private val LINK = Regex("\\[([^\\]]*)\\]\\(((?:[^()\\s]|\\([^()\\s]*\\))*)\\)")
    private val URL = Regex("$UNICODE(?<![\\w/@.])((?:https?://|www\\.)[^\\s<\\x{E000}\\x{E001}]+)")
    private val EMAIL = Regex("$UNICODE(?<![\\w.+-])([A-Za-z0-9._+-]+@[A-Za-z0-9_-]+(?:\\.[A-Za-z0-9_-]+)+)")
    private val BARE_AMP = Regex("$UNICODE&(?!#?\\w+;)")
    private val BOLD_STAR = Regex("$UNICODE\\*\\*(?=\\S)(.+?)(?<=\\S)\\*\\*")
    private val BOLD_UNDER = Regex("(?<![A-Za-z0-9])__(?=\\S)(.+?)(?<=\\S)__(?![A-Za-z0-9])")
    private val EM_STAR = Regex("$UNICODE\\*(?=\\S)(.+?)(?<=\\S)\\*")
    private val EM_UNDER = Regex("(?<![A-Za-z0-9])_(?=\\S)(.+?)(?<=\\S)_(?![A-Za-z0-9])")
    private val HELD = Regex("\\x{E000}(\\d+)\\x{E001}")

    // the email wrapper, as web/src/components/composer/renderers/email-template.ts writes it
    private val TEMPLATE_HEAD = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<style>
  body { margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; font-size: 14px; line-height: 1.6; color: #222; background: #fff; }
  a { color: #0066cc; }
  pre { white-space: pre-wrap; word-break: break-word; }
  img { max-width: 100%; height: auto; }
  @media (prefers-color-scheme: dark) {
    body { background: #1a1a1a; color: #e0e0e0; }
    a { color: #5b9cf5; }
    blockquote { border-left-color: #555 !important; }
    hr { border-top-color: #444 !important; }
  }
</style>
</head>
<body>
<div style="max-width:600px;margin:0 auto;padding:16px">
"""
    private const val TEMPLATE_TAIL = "\n</div>\n</body>\n</html>"
}
