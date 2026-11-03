package jp.golia.mailrs.wire

/**
 * Marks beside the links the server flagged, and the question asked
 * before one opens.
 *
 * The server names the hosts (`link_warnings` on each message); this
 * finds the matching anchors with the web client's rule: an anchor falls
 * under a warning when it goes to the warning's host and, for a warning
 * about the text showing another host, its text shows that host.
 *
 * The body is rendered with JavaScript off, so the mark and the question
 * cannot live in the page. A flagged anchor's target is swapped for
 * `mailrs-link-warning:<n>` and the real one kept here; the body hands
 * that address back and the reading screen asks first. Same as iOS.
 */
object LinkWarnings {
    const val SCHEME = "mailrs-link-warning"

    data class Target(val url: String, val warning: WireVerdict.LinkWarning)

    /** The body with its flagged anchors marked, and what each mark stands for. */
    data class Marked(val html: String, val targets: List<Target>)

    /** The warning an anchor going to [href] with the text [text] falls under. */
    fun warning(href: String, text: String, warnings: List<WireVerdict.LinkWarning>): WireVerdict.LinkWarning? {
        val host = hostOf(href) ?: return null
        val shownText = text.filterNot { it.isWhitespace() }.lowercase()
        return warnings.firstOrNull { w ->
            w.host == host && (w.shown == null || shownText.contains(w.shown))
        }
    }

    /** The index a body link stands for, when it is one of the marks. */
    fun index(url: String): Int? {
        if (!url.startsWith("$SCHEME:")) return null
        return url.removePrefix("$SCHEME:").toIntOrNull()
    }

    private val ANCHOR = Regex("""<a\b([^>]*)>(.*?)</a\s*>""", setOf(RegexOption.IGNORE_CASE, RegexOption.DOT_MATCHES_ALL))
    private val HREF = Regex("""\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))""", RegexOption.IGNORE_CASE)
    private val TAG = Regex("<[^>]*>")

    // the body's own stylesheet does not know the mark, so it carries its look
    private const val MARK_STYLE =
        "display:inline-block;margin-left:2px;padding:0 4px;border-radius:3px;" +
            "background:#dc2626;color:#fff;font:600 10px/14px sans-serif;" +
            "text-decoration:none;vertical-align:1px"

    /** Mark every flagged anchor in [html]; [describe] words a warning's reasons. */
    fun mark(
        html: String,
        warnings: List<WireVerdict.LinkWarning>,
        describe: (WireVerdict.LinkWarning) -> String,
    ): Marked {
        if (warnings.isEmpty()) return Marked(html, emptyList())
        val targets = mutableListOf<Target>()
        val out = ANCHOR.replace(html) { m ->
            val attrs = m.groupValues[1]
            val inner = m.groupValues[2]
            val h = HREF.find(attrs) ?: return@replace m.value
            val raw = h.groupValues.drop(1).firstOrNull { it.isNotEmpty() } ?: return@replace m.value
            val href = entities(raw)
            val w = warning(href, entities(TAG.replace(inner, "")), warnings) ?: return@replace m.value
            val n = targets.size
            targets += Target(href, w)
            val rewritten = attrs.replaceRange(h.range, "href=\"$SCHEME:$n\"")
            val label = escape(describe(w))
            "<a$rewritten>$inner</a>" +
                "<a href=\"$SCHEME:$n\" class=\"mailrs-link-warning\" role=\"img\"" +
                " aria-label=\"$label\" title=\"$label\" style=\"$MARK_STYLE\">!</a>"
        }
        if (targets.isEmpty()) return Marked(html, emptyList())
        return Marked(out, targets)
    }

    // mail content: a malformed address is ordinary, not an error
    private fun hostOf(href: String): String? =
        runCatching { java.net.URI(href).host?.lowercase() }.getOrNull()

    private fun entities(s: String): String = s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")

    private fun escape(s: String): String = s
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
}
