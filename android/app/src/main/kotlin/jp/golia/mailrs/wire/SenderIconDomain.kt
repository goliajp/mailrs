package jp.golia.mailrs.wire

/**
 * Which domain, if any, is worth asking the server for an icon.
 *
 * `GET /api/icon/{domain}` walks BIMI, then two favicon services, and
 * caches both hits and misses — but each first ask for a domain is a
 * live cascade with a four-second budget, so asking for something that
 * cannot have an icon costs a real request and answers 204.
 *
 * The handler itself rejects anything that is not shaped like a
 * hostname; this is the same rule on the near side, so a list of forty
 * senders does not send forty requests to be told no.
 */
object SenderIconDomain {

    fun of(sender: String): String? {
        val address = SenderIdentity.emailOf(sender)
        val domain = address.substringAfter('@', "").trim().lowercase()
        if (domain.isEmpty() || domain.length > 253) return null
        // A hostname, and one with a dot in it: `localhost` and the
        // bare machine names a self-hosted server sees have no favicon
        // anywhere and no BIMI record to find.
        if (!domain.contains('.')) return null
        if (domain.any { !(it.isLetterOrDigit() && it.code < 128) && it != '.' && it != '-' }) return null
        if (domain.startsWith('.') || domain.endsWith('.') || domain.contains("..")) return null
        return domain
    }
}
