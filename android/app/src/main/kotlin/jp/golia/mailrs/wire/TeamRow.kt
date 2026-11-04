package jp.golia.mailrs.wire

/** What a team conversation's row says about who answers it; the same rule as the iPhone's `TeamRow`. */
object TeamRow {
    /**
     * The assignee's readable name, or null when nobody is assigned. The
     * wire carries the address; the row shows the part before the `@`.
     */
    fun assignee(c: Wire.Conversation): String? {
        val who = c.assignee.trim()
        if (who.isEmpty()) return null
        val at = who.indexOf('@')
        if (at <= 0) return who
        return who.substring(0, at)
    }
}
