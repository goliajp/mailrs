package jp.golia.mailrs.wire

import jp.golia.mailrs.accounts.HtmlText

/**
 * Phone numbers a message asks the reader to call, and which of them the
 * same sender has not written before in this conversation.
 *
 * A number proves nothing about who answers it, so this only points it
 * out; it never judges the message. The rule and its limits are the web
 * client's, so the three clients point at the same numbers.
 */
object CallbackNumbers {
    // a leading `+`, or digit groups joined by spaces, dots, dashes or
    // brackets; the digit count decides below
    private val CANDIDATE = Regex("""(?:\+|\(\d)?\d[\d\s.()\-‐－]{7,22}\d""")
    private val GROUPING = Regex("""[\s.()\-‐－]""")
    private val DATE_LIKE = Regex("""^\d{4}[-.]\d{1,2}[-.]\d{1,2}""")
    private const val MIN_DIGITS = 10
    private const val MAX_DIGITS = 15
    private const val MAX_SHOWN = 3

    /**
     * Numbers in [message] that no earlier message from the same address
     * in [earlier] carried: a number a correspondent always signs with is
     * not worth pointing out, a new one asking to be called is.
     */
    fun new(message: Wire.Message, earlier: List<Wire.Message>): List<String> {
        val own = message.sender.lowercase()
        val before = earlier
            .filter { it.sender.lowercase() == own }
            .flatMap { phoneNumbers(plainText(it)).map(::digits) }
            .toSet()
        return phoneNumbers(plainText(message))
            .filterNot { digits(it) in before }
            .take(MAX_SHOWN)
    }

    /** Numbers in [text] that read as phone numbers, as written, once each. */
    fun phoneNumbers(text: String): List<String> {
        val seen = mutableSetOf<String>()
        val out = mutableListOf<String>()
        for (match in CANDIDATE.findAll(text)) {
            val written = match.value.trim()
            val d = digits(written)
            if (d.length < MIN_DIGITS || d.length > MAX_DIGITS) continue
            // an unbroken run is an order or account number, not a number to dial
            val grouped = written.startsWith("+") || GROUPING.containsMatchIn(written)
            // dates and times written with dots or dashes: 2026-11-01 12.30
            val dateLike = DATE_LIKE.containsMatchIn(written)
            if (!grouped || dateLike || !seen.add(d)) continue
            out += written
        }
        return out
    }

    private fun digits(s: String): String = s.filter { it in '0'..'9' }

    private fun plainText(m: Wire.Message): String {
        val text = m.textBody
        if (!text.isNullOrEmpty()) return text
        val html = m.htmlBody
        if (html.isNullOrEmpty()) return ""
        return HtmlText.plain(html)
    }
}
