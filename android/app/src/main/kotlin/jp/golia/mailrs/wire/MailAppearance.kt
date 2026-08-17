package jp.golia.mailrs.wire

/**
 * Whether a message's HTML may follow the app's appearance.
 *
 * Ported from `ios/Mailrs/Wire/MailAppearance.swift` so the two clients
 * cannot drift into disagreeing about the same question. The reasoning
 * is theirs and holds here: mail is authored against white, and handing
 * a dark background to a message that sets its own black text produces
 * black on black — worse than a bright rectangle. But that only covers
 * mail that styles itself. Most personal mail is a paragraph and a
 * link, and for that the white slab is just a bright rectangle in a
 * dark room.
 *
 * So the rule is narrow and conservative: a message that declares any
 * colour of its own — a background, a text colour, a `<font>` tag — is
 * a design, and its design is honoured on white paper. Only mail that
 * declares no colour at all inherits the app's.
 */
object MailAppearance {

    fun followsAppTheme(html: String): Boolean {
        val lowered = html.lowercase()
        if (lowered.contains("bgcolor")) return false
        if (lowered.contains("<font")) return false
        if (lowered.contains("background")) return false
        return !declaresTextColor(lowered)
    }

    /**
     * `color:` as its own property, not the tail of `border-color:` or
     * `outline-color:` — a border colour says nothing about whether the
     * text will be legible.
     */
    private fun declaresTextColor(lowered: String): Boolean {
        var from = 0
        while (true) {
            val at = lowered.indexOf("color:", from)
            if (at < 0) return false
            val precededByName = at > 0 && (lowered[at - 1] == '-' || lowered[at - 1].isLetter())
            if (!precededByName) return true
            from = at + "color:".length
        }
    }
}

/**
 * Whether a message reaches off the device to render.
 *
 * Ported from `ios/Mailrs/Wire/RemoteContent.swift`. Every remote
 * reference in mail is a beacon whether or not it was meant as one:
 * fetching it tells the sender the message was opened, from which
 * address, at what time, on what network.
 *
 * Deliberately generous about what counts — a missed reference is a
 * silent leak, while a false positive is one banner on a message that
 * had nothing to load.
 */
object RemoteContent {

    private val MARKERS = listOf(
        "src=\"http", "src='http", "src=http",
        "src=\"//", "src='//",
        "background=\"http", "background='http",
        "url(http", "url('http", "url(\"http",
        "url(//", "url('//", "url(\"//",
    )

    fun hasRemoteReferences(html: String): Boolean {
        val lowered = html.lowercase()
        return MARKERS.any { lowered.contains(it) }
    }
}
