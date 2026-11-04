package jp.golia.mailrs.wire

import jp.golia.mailrs.accounts.HtmlText
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Phone numbers a message asks the reader to call, and which of them the
 * same sender has not written before in this conversation.
 *
 * A number proves nothing about who answers it, so this only points it
 * out; it never judges the message. The rule and its limits are the web
 * client's, so the three clients point at the same numbers: the labels
 * come from `callback-numbers.json`, the web's table shipped in the app,
 * and the samples in the same file are what all three are tested against.
 */
object CallbackNumbers {
    // One phone number, by its structure: an optional `+` and country code,
    // an optional bracketed area code, then digit groups of at most six
    // joined by one space, dot or hyphen each. A digit, letter or hyphen on
    // either side means the digits belong to something else
    // (2026-INV-003001-2), as do a currency sign, a thousands comma or a
    // decimal point before them. A line break is not a separator.
    private val CANDIDATE = Regex(
        """(?<![A-Za-z0-9_+.,/#\x24€£¥\-‐－])""" +
            """(?:\+\d{1,3}[  .\-‐－]?)?(?:\(\d{1,4}\)[  .\-‐－]?)?""" +
            """\d{1,6}(?:[  .\-‐－]\d{1,6}){1,5}""" +
            """(?![A-Za-z0-9_:\-‐－]|[.,/]\d)""",
    )

    // dates and times written with dots, dashes or spaces: 2026-11-01 12.30
    private val DATE_LIKE = Regex("""^\d{4}[-./ ‐－]\d{1,2}[-./ ‐－]\d{1,2}(?!\d)""")

    // 1 234 567 890: an amount grouped by thousands, not a number to dial
    private val THOUSANDS = Regex("""^[1-9]\d{0,2}(?:[  .]\d{3})+$""")
    private const val LABEL_WINDOW = 40
    private const val MIN_DIGITS = 10

    // a national number with no country code is at most 11 digits in the
    // numbering plans this has to read (JP, US, UK)
    private const val MAX_NATIONAL_DIGITS = 11
    private const val MAX_DIGITS = 15
    private const val MAX_SHOWN = 3

    /** The labels in the table the app ships. */
    val labels: List<String> by lazy {
        // the file ships inside the app, and the unit tests read this same copy
        val text = CallbackNumbers::class.java.getResource("/callback-numbers.json")!!.readText()
        Json.parseToJsonElement(text).jsonObject.getValue("labels").jsonArray.map { it.jsonPrimitive.content }
    }

    // digits straight after one of these name an order, invoice or account
    private val AFTER_LABEL by lazy {
        Regex(
            "(?:^|[^A-Za-z0-9])(?:" +
                labels.sortedByDescending { it.length }.joinToString("|") { Regex.escape(it) } +
                """)[\s:：.#＃№\-]*$""",
            RegexOption.IGNORE_CASE,
        )
    }

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

    /**
     * Numbers in [text] that read as phone numbers, as written, once each.
     * Misses some real ones on purpose: the notice is worth reading only
     * while it is almost never wrong.
     */
    fun phoneNumbers(text: String): List<String> {
        val seen = mutableSetOf<String>()
        val out = mutableListOf<String>()
        for (match in CANDIDATE.findAll(text)) {
            val written = match.value
            val d = digits(written)
            if (d in seen || !dialable(written, d)) continue
            val start = match.range.first
            if (AFTER_LABEL.containsMatchIn(text.substring(maxOf(0, start - LABEL_WINDOW), start))) continue
            seen += d
            out += written
        }
        return out
    }

    private fun dialable(written: String, d: String): Boolean {
        if (d.length < MIN_DIGITS || d.length > maxDigits(written)) return false
        return !DATE_LIKE.containsMatchIn(written) && !THOUSANDS.containsMatchIn(written)
    }

    private fun maxDigits(written: String): Int {
        if (written.startsWith("+")) return MAX_DIGITS
        return MAX_NATIONAL_DIGITS
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
