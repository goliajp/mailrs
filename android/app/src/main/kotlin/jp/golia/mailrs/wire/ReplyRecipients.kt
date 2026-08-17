package jp.golia.mailrs.wire

/**
 * Who a reply goes to — the web's rules (`thread-view.tsx`), ported the
 * same way `ios/Mailrs/Wire/ReplyRecipients.swift` ports them.
 *
 * - **Reply**: the sender of the message being answered.
 * - **Reply-all**: the sender plus everyone on the To line, minus
 *   yourself — a reply must not arrive addressed back at the person
 *   sending it.
 * - Subjects gain `Re:` / `Fwd:` unless already carrying one, matched
 *   case-insensitively: `RE: x` must not become `Re: RE: x`.
 *
 * Pure, so the rules are testable without a screen or a server. The
 * three clients agreeing here is what stops one of them quietly
 * dropping a cc.
 */
object ReplyRecipients {

    fun reply(sender: String): List<String> = listOf(SenderIdentity.emailOf(sender))

    /**
     * `recipients` is the wire's comma/semicolon-joined To line, whose
     * entries may be display forms. Order is sender first, then the To
     * line in its own order; duplicates collapse to first appearance.
     */
    fun replyAll(sender: String, recipients: String, myAddress: String): List<String> {
        val mine = myAddress.lowercase()
        val seen = LinkedHashSet<String>()
        val all = listOf(sender) + recipients.split(',', ';')
        for (entry in all) {
            val email = SenderIdentity.emailOf(entry.trim())
            if (email.isEmpty() || email == mine) continue
            seen.add(email)
        }
        return seen.toList()
    }

    fun subject(original: String, forwarding: Boolean = false): String {
        val prefix = if (forwarding) "Fwd:" else "Re:"
        if (original.lowercase().startsWith(prefix.lowercase())) return original
        if (original.isEmpty()) return prefix
        return "$prefix $original"
    }

    /**
     * The quoted history a reply opens with — the attribution line and
     * the original indented, which is what every other client does and
     * what makes a reply readable to someone who did not keep the
     * thread.
     */
    fun quote(sender: String, sentAt: Long, body: String): String {
        val who = SenderIdentity.readableName(sender)
        val quoted = body.trim().lines().joinToString("\n") { "> $it" }
        return "\n\nOn ${RowDateText.of(sentAt)}, $who wrote:\n$quoted\n"
    }
}

/**
 * The one date format the quote line uses.
 *
 * Separate from the list's `RowDate` ladder on purpose: a quote is read
 * later and out of context, so it says the whole date rather than
 * "14:32", which means nothing three replies down.
 */
object RowDateText {
    fun of(epochSeconds: Long): String {
        if (epochSeconds <= 0) return "an earlier date"
        val moment = java.time.Instant.ofEpochSecond(epochSeconds).atZone(java.time.ZoneId.systemDefault())
        return java.time.format.DateTimeFormatter
            .ofLocalizedDateTime(java.time.format.FormatStyle.MEDIUM, java.time.format.FormatStyle.SHORT)
            .withLocale(java.util.Locale.getDefault())
            .format(moment)
    }
}
