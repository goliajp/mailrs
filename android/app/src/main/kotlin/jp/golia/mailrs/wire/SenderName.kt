package jp.golia.mailrs.wire

import jp.golia.mailrs.R
import jp.golia.mailrs.accounts.EncodedWord

/**
 * Turning a From header into the name a row shows.
 *
 * Ported from `ios/Mailrs/Wire/SenderName.swift` (itself from the web's
 * `lib/avatar.ts`), rule for rule and with the same test cases, so the
 * three clients put the same face on the same mail: encoded words
 * decoded, `"Name" <addr>` yields the name, a bare address yields its
 * local part, and a machine-generated local part (bounce chains, VERP,
 * hashes) yields the brand from the domain instead.
 */
object SenderName {
    fun extractEmail(sender: String): String {
        val decoded = decode(sender)
        val inAngles = Regex("<([^>]+)>").find(decoded)?.groupValues?.get(1)
        return (inAngles ?: decoded).lowercase()
    }

    fun extractName(sender: String): String {
        val decoded = decode(sender)
        Regex("^\"?([^\"<]+)\"?\\s*<").find(decoded)?.let { match ->
            val name = match.groupValues[1].trim()
            if (!name.contains('@') && !isMachineGenerated(name)) return name
        }
        val email = extractEmail(decoded)
        val at = email.indexOf('@')
        if (at < 0) {
            if (email.isEmpty()) return sender
            return email
        }
        val local = email.substring(0, at)
        if (isMachineGenerated(local)) {
            return domainLabel(email.substring(at + 1)).replaceFirstChar { it.uppercase() }
        }
        return local
    }

    /** Tracking ids, bounce chains, VERP addresses, hashes. */
    fun isMachineGenerated(s: String): Boolean {
        if (s.length <= 10) return false
        val lowered = s.lowercase()
        if (lowered.startsWith("bounce") || lowered.startsWith("msprvs") || lowered.startsWith("prvs")) return true
        if (s.length > 15 && s.contains('=')) return true
        val digits = s.count { it.isDigit() }
        if (s.length > 12 && digits.toDouble() / s.length > 0.3) return true
        if (s.length > 20 && !s.contains(' ') && digits > 0) return true
        val letters = s.count { it in 'a'..'z' || it in 'A'..'Z' }
        if (s.length > 15 && letters.toDouble() / s.length < 0.5) return true
        return false
    }

    private val SECONDARY_TLDS = setOf("ac", "co", "com", "edu", "gov", "ne", "net", "or", "org")

    /** `notify.cloudflare.com` → `cloudflare`; `example.co.jp` → `example`. */
    fun domainLabel(domain: String): String {
        val parts = domain.split('.')
        if (parts.size < 2) return domain
        val sld = parts[parts.size - 2]
        if (parts.size >= 3 && sld in SECONDARY_TLDS) return parts[parts.size - 3]
        return sld
    }

    /** The participants other than the reader, in the server's order. */
    fun others(participants: List<String>, myAddress: String): List<String> {
        val mine = myAddress.lowercase()
        return participants.filter { extractEmail(it) != mine }
    }

    /**
     * The name a conversation row wears: the first participant who is not
     * the reader. After a reply the reader's own address can come first,
     * and a row wearing it reads like a sent mail sitting in the inbox.
     * Only a thread with nobody else on it says "Me".
     */
    fun rowFace(participants: List<String>, myAddress: String): String {
        others(participants, myAddress).firstOrNull()?.let { return extractName(it) }
        if (participants.isEmpty()) return Texts.get(R.string.mail_no_sender)
        return Texts.get(R.string.sender_me)
    }

    private fun decode(value: String): String =
        EncodedWord.decode(value).replace(Regex("\\s+"), " ").trim()
}
