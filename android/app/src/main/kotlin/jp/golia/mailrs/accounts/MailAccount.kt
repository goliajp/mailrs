package jp.golia.mailrs.accounts

import kotlinx.serialization.Serializable

/**
 * A mailbox somewhere else, as this app holds it.
 *
 * The credential is **not** here: it lives in encrypted storage under
 * the account's id, so a row can be logged, encoded, or shown on
 * screen without carrying a password through code that has no
 * business holding one.
 */
@Serializable
data class MailAccount(
    val id: String,
    /**
     * The address, and what the server knows this person as unless
     * [login] says otherwise.
     */
    val address: String,
    /** What to call it on screen. Empty means show the address. */
    val displayName: String = "",
    /** A preset id — `gmail`, `qq` — or `custom`. */
    val provider: String = "custom",
    val imapHost: String = "",
    val imapPort: Int = 993,
    val smtpHost: String = "",
    val smtpPort: Int = 465,
    /**
     * The login name, when the server wants something other than the
     * address. Empty means the address.
     */
    val login: String = "",
    val auth: MailProvider.AuthKind = MailProvider.AuthKind.PASSWORD,
    val skipFolders: List<String> = emptyList(),
    /** Where it sits in the list. */
    val sort: Int = 0,
) {
    /** What the server is told to call this person. */
    val loginName: String get() = login.ifEmpty { address }

    /** What a person sees. */
    val title: String get() = displayName.ifEmpty { address }

    /**
     * The second line of a row, or null.
     *
     * An account with no name of its own falls back to its address on
     * the first line, so repeating the address underneath says nothing
     * and reads as a rendering fault.
     */
    val subtitle: String? get() =
        if (displayName.isEmpty() || displayName == address) null else address

    /**
     * What is wrong with this account, in words a person can act on.
     *
     * Checked here rather than at the server so a set-up screen can say
     * what is missing before spending thirty seconds finding out that a
     * blank host does not resolve.
     */
    val problem: String? get() = when {
        !address.contains('@') || address.startsWith('@') || address.endsWith('@') ->
            "That is not an email address"
        imapHost.isBlank() -> "The incoming server needs a name"
        smtpHost.isBlank() -> "The outgoing server needs a name"
        imapPort !in 1..65535 || smtpPort !in 1..65535 ->
            "A port must be between 1 and 65535"
        else -> null
    }

    companion object {
        /**
         * A stable id for an address.
         *
         * Derived rather than random so adding the same account twice
         * is the same row rather than two — and so a stored credential
         * survives a list rebuilt from scratch.
         */
        fun idFor(address: String): String {
            var h = 0xcbf29ce484222325uL
            for (c in address.lowercase()) {
                h = h xor c.code.toULong()
                h *= 0x100000001b3uL
            }
            return "acct-$h"
        }

        /** A row filled in from what is known about the address. */
        fun make(address: String, displayName: String = "", sort: Int = 0): MailAccount {
            val known = MailProvider.forAddress(address)
            val domain = address.substringAfterLast('@', "")
            val p = known ?: MailProvider.guess(domain)
            return MailAccount(
                id = idFor(address),
                address = address,
                displayName = displayName,
                provider = if (known == null) "custom" else p.label.lowercase(),
                imapHost = p.imapHost, imapPort = p.imapPort,
                smtpHost = p.smtpHost, smtpPort = p.smtpPort,
                auth = p.auth,
                skipFolders = p.skipFolders,
                sort = sort,
            )
        }

        /**
         * A colour per mailbox, so a merged list can say which is which.
         *
         * Derived from the id rather than stored: the same account is
         * the same colour on every launch, and there is nothing to keep
         * in step.
         */
        val palette = listOf(
            0xFF4285F4, 0xFF12B7F5, 0xFFEA4335, 0xFF34A853,
            0xFFA142F4, 0xFFF4B400, 0xFFFF6D00, 0xFF00897B,
        )

        fun colourFor(id: String): Long {
            var h = 0xcbf29ce484222325uL
            for (b in id.encodeToByteArray()) {
                h = h xor b.toUByte().toULong()
                h *= 0x100000001b3uL
            }
            return palette[(h % palette.size.toULong()).toInt()]
        }
    }
}
