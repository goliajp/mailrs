package jp.golia.mailrs.wire

/**
 * Which messages in a thread show their full card; the same rule as the
 * iPhone's `Wire/ThreadCollapse.swift`.
 *
 * A long thread opens with everything but the newest message folded to a
 * header line: the thread is context, the last message is the reason you
 * came. The state is only the set of messages the reader toggled, and a
 * message shows expanded when (it is the last) XOR (it was toggled), so a
 * new thread needs no reconciling: the set starts empty and the rule is
 * right again.
 */
object ThreadCollapse {
    fun isExpanded(uid: Int, lastUid: Int?, toggled: Set<Int>): Boolean =
        (uid == lastUid) != toggled.contains(uid)

    /** The folded card's one line of body: whitespace collapsed, hard bounded. */
    fun snippet(text: String?, limit: Int = 80): String {
        if (text == null) return ""
        val folded = text.replace(Regex("\\s+"), " ").trim()
        if (folded.length <= limit) return folded
        return folded.take(limit) + "…"
    }
}
