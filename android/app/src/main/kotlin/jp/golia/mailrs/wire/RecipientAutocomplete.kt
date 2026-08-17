package jp.golia.mailrs.wire

/**
 * The token arithmetic under a recipient field's suggestions.
 *
 * Ported from `ios/Mailrs/Wire/RecipientAutocomplete.swift`. A
 * recipient line is comma/semicolon-separated; suggestions apply to the
 * token the cursor is in — in practice the last one, since address entry
 * is append-shaped. Completing replaces that token with the picked
 * contact's bare address and leaves a separator ready for the next.
 */
object RecipientAutocomplete {

    /**
     * What is being typed: the text after the last separator, trimmed.
     * Empty when the person has just finished an entry.
     */
    fun currentToken(text: String): String {
        val last = text.indexOfLast { it == ',' || it == ';' }
        if (last < 0) return text.trim()
        return text.substring(last + 1).trim()
    }

    /**
     * Whether a query is worth a request: two characters, the same floor
     * as search, and not already a complete address — suggesting for
     * "alice@example.com" answers a question that was already answered.
     */
    fun shouldSuggest(token: String): Boolean = token.length >= 2 && !token.contains("@")

    /** The line with the in-progress token replaced, ready for the next. */
    fun completing(text: String, contact: String): String {
        val email = SenderIdentity.emailOf(contact)
        val last = text.indexOfLast { it == ',' || it == ';' }
        if (last < 0) return "$email, "
        return text.substring(0, last + 1) + " " + email + ", "
    }
}
