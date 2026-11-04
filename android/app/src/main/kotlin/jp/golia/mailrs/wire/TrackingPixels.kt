package jp.golia.mailrs.wire

import android.content.Context
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/** The parts of `trackers.json` that pick out a tracking image. */
@Serializable
data class TrackerRules(val hosts: List<String>, val paths: List<String>) {
    companion object {
        private val json = Json { ignoreUnknownKeys = true }

        fun parse(text: String): TrackerRules = json.decodeFromString(serializer(), text)

        @Volatile private var shipped: TrackerRules? = null

        /** The copy the app ships, read once. */
        fun shipped(context: Context): TrackerRules {
            shipped?.let { return it }
            val read = parse(context.assets.open("trackers.json").bufferedReader().use { it.readText() })
            shipped = read
            return read
        }
    }
}

/**
 * The images in a received message that only tell the sender it was read.
 *
 * The web's rule, from the same `trackers.json` the server and the web read
 * (a copy ships in the app's assets; `check-tracker-parity.sh` keeps the bytes
 * identical): an `http(s)` image is a tracker when its host or path is a known
 * tracking one, or when it is invisible — hidden by style, or every side it
 * states is a pixel or less. The iPhone's `Wire/TrackingPixels.swift` is the
 * same code.
 *
 * Taken out of the document, not merely blocked: "show images" is a decision
 * about pictures, not about being counted.
 */
object TrackingPixels {
    private val imgOpen = Regex("<img", RegexOption.IGNORE_CASE)
    private val scheme = Regex("^https?://", RegexOption.IGNORE_CASE)
    private val attribute = Regex("""([^\s"'>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?""")
    private val pixels = Regex("""^\s*(\d+)\s*(px)?\s*$""", RegexOption.IGNORE_CASE)

    /** The document with its trackers taken out. */
    fun strip(html: String, rules: TrackerRules): String {
        if (!html.contains("<img", ignoreCase = true)) return html
        val out = StringBuilder(html.length)
        var at = 0
        while (true) {
            val open = imgOpen.find(html, at) ?: break
            out.append(html, at, open.range.first)
            val close = tagEnd(html, open.range.first)
            if (close < 0) {
                out.append(html, open.range.first, html.length)
                return out.toString()
            }
            val tag = html.substring(open.range.first, close + 1)
            if (!isTracker(tag, rules)) out.append(tag)
            at = close + 1
        }
        out.append(html, at, html.length)
        return out.toString()
    }

    /** How many trackers [strip] takes out, for the note above the message. */
    fun count(html: String, rules: TrackerRules): Int {
        var n = 0
        var at = 0
        while (true) {
            val open = imgOpen.find(html, at) ?: return n
            val close = tagEnd(html, open.range.first)
            if (close < 0) return n
            if (isTracker(html.substring(open.range.first, close + 1), rules)) n++
            at = close + 1
        }
    }

    /** Whether one `<img …>` tag is a tracker. */
    fun isTracker(tag: String, rules: TrackerRules): Boolean {
        val attrs = attributes(tag)
        val src = (attrs["src"] ?: "").trim()
        if (!scheme.containsMatchIn(src)) return false
        return trackingUrl(src, rules) || invisible(attrs)
    }

    /** The `>` that closes the tag, skipping any inside a quoted value; -1 when none. */
    private fun tagEnd(html: String, from: Int): Int {
        var quote = ' '
        for (i in from until html.length) {
            val c = html[i]
            if (quote != ' ') {
                if (c == quote) quote = ' '
            } else if (c == '"' || c == '\'') {
                quote = c
            } else if (c == '>') {
                return i
            }
        }
        return -1
    }

    /** Attribute names lowercased; the first of a repeated name wins, as in a browser. */
    private fun attributes(tag: String): Map<String, String> {
        val inner = tag.substring(4, tag.length - 1)
        val out = HashMap<String, String>()
        for (m in attribute.findAll(inner)) {
            val name = m.groupValues[1].lowercase()
            if (name in out) continue
            out[name] = listOf(2, 3, 4).firstNotNullOfOrNull { m.groups[it]?.value } ?: ""
        }
        return out
    }

    private fun trackingUrl(src: String, rules: TrackerRules): Boolean {
        val uri = runCatching { java.net.URI(src) }.getOrNull() ?: return false
        val host = uri.host?.lowercase() ?: return false
        val path = (uri.rawPath ?: "").lowercase()
        if (rules.hosts.any { host == it || host.endsWith(".$it") }) return true
        return rules.paths.any { path.contains(it) }
    }

    /** Drawn at a pixel or less on each side it states, or hidden outright. */
    private fun invisible(attrs: Map<String, String>): Boolean {
        val style = (attrs["style"] ?: "").lowercase().filterNot { it.isWhitespace() }
        if (style.contains("display:none") || style.contains("visibility:hidden")) return true
        val sides = listOf("width", "height").mapNotNull { side -> px(attrs[side]) ?: fromStyle(style, side) }
        return sides.isNotEmpty() && sides.all { it <= 1 }
    }

    private fun fromStyle(style: String, side: String): Int? =
        Regex("""(?:^|;)$side:(\d+)(px)?(?:;|$)""").find(style)?.groupValues?.get(1)?.toIntOrNull()

    /** `1`, `1px`, `0`; null for anything relative or absent. */
    private fun px(value: String?): Int? {
        if (value == null) return null
        return pixels.find(value)?.groupValues?.get(1)?.toIntOrNull()
    }
}
