package jp.golia.mailrs.wire

/**
 * The composer toolbar's bold, italic and link, applied to the text and the
 * selection the way the web composer does (`web/src/lib/markdown-format.ts`)
 * and iOS does (`Wire/MarkdownFormat.swift`): the same marks, so a message
 * reads the same whichever client wrote it. Offsets are `String` indices.
 */
object MarkdownFormat {
    enum class Kind { Bold, Italic, Link }

    data class Edit(val value: String, val start: Int, val end: Int)

    fun apply(value: String, start: Int, end: Int, kind: Kind): Edit {
        val lower = minOf(start, end).coerceIn(0, value.length)
        val upper = maxOf(start, end).coerceIn(lower, value.length)
        return when (kind) {
            Kind.Bold -> wrap(value, lower, upper, "**")
            Kind.Italic -> wrap(value, lower, upper, "_")
            Kind.Link -> link(value, lower, upper)
        }
    }

    // wrapping an already wrapped selection takes the marks off again
    private fun wrap(value: String, start: Int, end: Int, mark: String): Edit {
        val before = value.substring(0, start)
        val picked = value.substring(start, end)
        val after = value.substring(end)
        if (before.endsWith(mark) && after.startsWith(mark)) {
            return Edit(
                before.dropLast(mark.length) + picked + after.drop(mark.length),
                start - mark.length,
                end - mark.length,
            )
        }
        return Edit(before + mark + picked + mark + after, start + mark.length, end + mark.length)
    }

    // the link text stays as typed and the url placeholder is selected, ready to paste over
    private fun link(value: String, start: Int, end: Int): Edit {
        val url = "https://"
        val head = value.substring(0, start) + "[" + value.substring(start, end) + "]("
        return Edit(head + url + ")" + value.substring(end), head.length, head.length + url.length)
    }
}
