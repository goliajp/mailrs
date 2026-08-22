package jp.golia.mailrs.wire

/**
 * The two decisions the account screens make before any request.
 */

/**
 * Whether this is enough of an address to look a provider up.
 *
 * A partial address is not a domain, and asking about "s", "so", "som"
 * is three requests that cannot answer anything.
 */
fun looksLikeAnAddress(v: String): Boolean {
    val parts = v.trim().split("@")
    return parts.size == 2 && parts[0].isNotEmpty() && parts[1].contains(".") &&
        !parts[1].startsWith(".") && !parts[1].endsWith(".")
}

/**
 * The account dot's colour, as an ARGB int.
 *
 * The server chooses it so all three clients agree on which dot means
 * which mailbox. Anything unreadable falls back to a neutral grey: a
 * row with no dot reads as a different kind of account, and a crash
 * over a colour would be worse than either.
 */
fun colourOf(hex: String?): Int {
    val cleaned = hex?.removePrefix("#") ?: return GREY
    if (cleaned.length != 6) return GREY
    val v = cleaned.toLongOrNull(16) ?: return GREY
    return (0xFF000000L or v).toInt()
}

private const val GREY = 0xFF6B7280.toInt()
