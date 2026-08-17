package jp.golia.mailrs.wire

/**
 * What this app is willing to say about who sent a message.
 *
 * **Warnings only. There is no positive mark, and that is deliberate.**
 *
 * The other two clients each had one and each lost it on 2026-08-16. A
 * JCB phishing mail earned a green check on both: `spf=pass dkim=pass
 * dmarc=pass`, every check correct, because `wokjx.crabfishhh.com` is
 * the attacker's own domain and authentication records are free on a
 * domain you control. DMARC's claim is that the mail came from the
 * domain in the From header; the lie was the display name, which DMARC
 * does not authenticate.
 *
 * Gmail's check mark requires DMARC at an enforced policy, BIMI, **and a
 * Verified Mark Certificate** — a CA verifying trademark ownership. It
 * stands for an identity checked out of band, never for a passing
 * authentication run. We have no VMC, so we show no mark.
 *
 * The asymmetry is the design: a warning that is sometimes wrong costs a
 * reader distrusting real mail; a mark that is sometimes wrong costs a
 * reader trusting a fake one, and the attacker chose it.
 */
object SenderIdentity {

    /**
     * Whether to warn about this message's sender.
     *
     * Only `suspicious`. `unverified` is the vast ordinary middle —
     * most legitimate mail from small senders lands there — and warning
     * about it teaches people to ignore the one verdict that matters.
     * `verified` is a true statement about authentication and is not
     * rendered at all.
     */
    fun isSuspicious(senderTrust: String): Boolean = senderTrust == "suspicious"

    /**
     * The domain the mail actually came from, when the display name says
     * a different one — or null when it claims nothing, or the two
     * agree.
     *
     * The header shows a display name and rarely an address, which is
     * the gap brand impersonation lives in: `Amazon.co.jp` reads as
     * Amazon whether it was sent by Amazon or by
     * `mail07.jqjintaiyang.com`. Measured on this deployment's 33,583
     * stored messages: 4,825 display names contain a domain-shaped
     * token, and **83** — 0.247% — name a domain that did not send it,
     * including `golia.jp | HR` sent from `halfwaylexus.cam`.
     *
     * It **states** rather than accuses. "Sent from X" is useful even
     * when X turns out to be the same company's second domain, so a
     * false positive costs the reader nothing — which is what makes it
     * safe on a signal that cannot be perfect.
     *
     * Companion to the character check on the server
     * (`mailrs_textguard`), not a duplicate of it: that one must not
     * flag `U+FEFF`, `U+200C`, `U+200D` or `U+00AD`, because they have
     * real typographic work to do — and three production phish use
     * exactly those to break up `Amazon` inside a display name. This
     * catches those.
     */
    fun contradictedDomain(sender: String): String? {
        val sending = domainOf(sender) ?: return null
        val claimed = claimedDomain(displayName(sender)) ?: return null
        if (agree(claimed, sending)) return null
        return sending
    }

    /**
     * Suffixes that make a token read as a domain. Deliberately a short
     * common list rather than the public suffix list: the point is to
     * catch what a person would read *as* a domain, and `amazon.co.jp`
     * in a display name is that whether or not a full PSL agrees about
     * its boundaries.
     */
    private val SUFFIXES = setOf(
        "ai", "app", "cn", "co", "com", "dev", "io", "jp", "me", "net", "org", "shop",
    )

    private fun displayName(sender: String): String {
        val angle = sender.indexOf('<')
        val name = if (angle >= 0) sender.substring(0, angle) else sender
        return name.trim().trim('"')
    }

    /** The address inside a `Name <addr>` header, lowercased. */
    fun emailOf(sender: String): String {
        val inAngles = Regex("<([^>]*)>").find(sender)?.groupValues?.get(1)
        return (inAngles ?: sender).trim().lowercase()
    }

    private fun domainOf(sender: String): String? {
        val inAngles = Regex("<([^>]*)>").find(sender)?.groupValues?.get(1)
        val email = (inAngles ?: sender).trim().lowercase()
        val at = email.lastIndexOf('@')
        if (at < 0) return null
        val domain = email.substring(at + 1)
        return if (domain.contains('.')) domain else null
    }

    /**
     * The first domain-shaped token in a display name.
     *
     * Splits on everything a DNS label cannot contain, so `【Amazon.co.jp】`
     * and a name padded with zero-width characters both yield the token —
     * those are separators here too.
     */
    fun claimedDomain(displayName: String): String? {
        for (raw in displayName.lowercase().split(Regex("[^0-9a-z.-]+"))) {
            val candidate = raw.trim('.', '-')
            val labels = candidate.split('.')
            if (labels.size < 2) continue
            if (labels.last() !in SUFFIXES) continue
            if (labels.dropLast(1).any { it.isEmpty() }) continue
            // A bare `co.jp` is a suffix, not a claim.
            if (labels.size == 2 && labels[0] in SUFFIXES) continue
            return candidate
        }
        return null
    }

    /**
     * Related enough that saying so would be noise. Mail from
     * `email.amazon.co.jp` for a name saying `amazon.co.jp` is the
     * ordinary case and must stay silent.
     */
    fun agree(claimed: String, sending: String): Boolean =
        claimed == sending || sending.endsWith(".$claimed") || claimed.endsWith(".$sending")

    /** The part of a From header a person reads, for display. */
    fun readableName(sender: String): String {
        val name = displayName(sender)
        if (name.isNotEmpty()) return name
        return domainOf(sender)?.let { sender.substringBefore('@') } ?: sender
    }
}
