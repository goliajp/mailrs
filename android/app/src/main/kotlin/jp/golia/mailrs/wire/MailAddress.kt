package jp.golia.mailrs.wire

/**
 * Whether [text] has the shape of a mailbox address: one `@`, something
 * before it, and a domain with a dot that is not its last character.
 *
 * A shape, not a verdict: the server decides whether the mailbox exists.
 * The same rule as `MailAddress.looksLike` on iOS.
 */
fun looksLikeAddress(text: String): Boolean {
    if (text.any { it.isWhitespace() }) return false
    val parts = text.split("@")
    if (parts.size != 2) return false
    val (local, domain) = parts
    return local.isNotEmpty() && domain.contains(".") && !domain.endsWith(".") && !domain.startsWith(".")
}
