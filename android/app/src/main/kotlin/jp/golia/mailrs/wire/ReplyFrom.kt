package jp.golia.mailrs.wire

/** One address a message can leave by. */
data class FromAddress(val accountId: String, val address: String, val label: String)

/**
 * Every address this person can send as, this server's own first.
 *
 * An account whose credential was refused is left out: choosing it
 * would produce a message that cannot be sent, and offering a choice
 * that fails is worse than not offering it.
 */
fun fromAddresses(own: String, accounts: List<Wire.ExternalAccount>): List<FromAddress> {
    val out = mutableListOf<FromAddress>()
    if (own.isNotEmpty()) out += FromAddress("", own, own)
    for (a in accounts) {
        if (a.state == "needs_auth") continue
        val label = when {
            a.displayName.isNotEmpty() && a.displayName != a.email -> "${a.displayName} · ${a.email}"
            else -> a.email
        }
        out += FromAddress(a.id, a.email, label)
    }
    return out
}

/**
 * The address a reply should leave by, given where the mail arrived.
 *
 * Not "the account you signed in as". A reply to mail that arrived at a
 * connected Gmail has to go out through that Gmail — sent from
 * anywhere else it lands in the conversation as a stranger, and half
 * the time the recipient's provider refuses it outright.
 *
 * Falls back to this server's address when the conversation came from
 * an account that is gone or cannot send: replying from somewhere
 * beats a composer that will not send.
 */
fun replyFromFor(accountId: String?, addresses: List<FromAddress>): String =
    addresses.firstOrNull { it.accountId == (accountId ?: "") }?.address
        ?: addresses.firstOrNull()?.address
        ?: ""
