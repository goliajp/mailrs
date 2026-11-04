package jp.golia.mailrs.wire

/**
 * The 1×1 images that exist only to report that a message was opened; the
 * same rule as the iPhone's `Wire/TrackingPixels.swift`.
 *
 * Taken out of the document, not merely blocked: "show images" is a
 * decision about pictures, not about being counted, and a beacon loaded
 * with them tells the sender the mail was read, from which network.
 */
object TrackingPixels {
    private val imgOpen = Regex("<img", RegexOption.IGNORE_CASE)
    private val inlineOne = Regex("""\bstyle\s*=\s*["'][^"']*\b(?:width|height)\s*:\s*1px""", RegexOption.IGNORE_CASE)

    /** The document with its beacons taken out. */
    fun strip(html: String): String {
        if (!html.contains("<img", ignoreCase = true)) return html
        val out = StringBuilder(html.length)
        var at = 0
        while (true) {
            val open = imgOpen.find(html, at) ?: break
            out.append(html, at, open.range.first)
            val close = html.indexOf('>', open.range.first)
            if (close < 0) {
                out.append(html, open.range.first, html.length)
                return out.toString()
            }
            val tag = html.substring(open.range.first, close + 1)
            if (!isBeacon(tag)) out.append(tag)
            at = close + 1
        }
        out.append(html, at, html.length)
        return out.toString()
    }

    /** How many beacons [strip] takes out, for the "N trackers blocked" note. */
    fun count(html: String): Int {
        var n = 0
        var at = 0
        while (true) {
            val open = imgOpen.find(html, at) ?: return n
            val close = html.indexOf('>', open.range.first)
            if (close < 0) return n
            if (isBeacon(html.substring(open.range.first, close + 1))) n++
            at = close + 1
        }
    }

    /** A tag is a beacon when it declares itself one pixel each way. */
    fun isBeacon(tag: String): Boolean {
        val width = one(tag, "width")
        val height = one(tag, "height")
        if (width && height) return true
        return inlineOne.containsMatchIn(tag)
    }

    private fun one(tag: String, name: String): Boolean =
        Regex("""\b$name\s*=\s*["']?\s*1\s*["']?""", RegexOption.IGNORE_CASE).containsMatchIn(tag)
}
