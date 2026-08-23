package jp.golia.mailrs.accounts

/**
 * The accounts screen's state, and the rules it applies before any
 * request goes out.
 *
 * A plain data class rather than a ViewModel so the rules can be
 * tested without an Android runtime — which is where they can be got
 * wrong, and where a test is worth having.
 */
data class AccountsDraft(
    val address: String = "",
    val secret: String = "",
    val name: String = "",
    /**
     * Opened only when somebody asks: a form that starts with five
     * empty boxes teaches everybody that connecting mail is hard.
     */
    val manual: Boolean = false,
    val imapHost: String = "",
    val imapPort: String = "",
    val smtpHost: String = "",
    val smtpPort: String = "",
    val login: String = "",
) {
    /**
     * A partial address is not a domain: looking one up for "s", "so",
     * "som" is three answers nobody asked for.
     */
    val addressLooksComplete: Boolean
        get() {
            val parts = address.split("@")
            return parts.size == 2 && parts[1].contains(".")
        }

    /** What is known about this address, once it is an address. */
    val provider: MailProvider?
        get() = if (addressLooksComplete) MailProvider.forAddress(address) else null

    /** The provider's own word for what to type, or the plain one. */
    val secretLabel: String get() = provider?.secretHelp?.what ?: "Password"

    /**
     * The two endpoints as typed, or null.
     *
     * Digits only for a port, deliberately: `"+993".toIntOrNull()` is
     * 993 on this platform, and that is not what somebody typing a
     * port means. Surrounding spaces are trimmed — a paste is not a
     * mistake.
     */
    fun endpoints(): Endpoints? {
        fun port(s: String): Int? {
            val t = s.trim()
            if (t.isEmpty() || !t.all { it.isDigit() }) return null
            val n = t.toIntOrNull() ?: return null
            return if (n in 1..65535) n else null
        }
        val ih = imapHost.trim()
        val sh = smtpHost.trim()
        if (ih.isEmpty() || sh.isEmpty()) return null
        val ip = port(imapPort) ?: return null
        val sp = port(smtpPort) ?: return null
        return Endpoints(ih, ip, sh, sp)
    }

    /**
     * The boxes, filled in from what is known.
     *
     * An empty form is one somebody has to research; a filled one is
     * one they correct.
     */
    fun prefilled(): AccountsDraft {
        if (!addressLooksComplete) return this
        val a = MailAccount.make(address)
        return copy(
            imapHost = imapHost.ifEmpty { a.imapHost },
            imapPort = imapPort.ifEmpty { a.imapPort.toString() },
            smtpHost = smtpHost.ifEmpty { a.smtpHost },
            smtpPort = smtpPort.ifEmpty { a.smtpPort.toString() },
        )
    }

    /** The account this draft describes, or the reason it cannot. */
    fun account(sort: Int): Result<MailAccount> {
        if (!addressLooksComplete) {
            return Result.failure(
                IllegalArgumentException("Enter the full email address of the account to add"),
            )
        }
        var account = MailAccount.make(address.trim(), name.trim(), sort)
        if (manual) {
            val e = endpoints()
                ?: return Result.failure(
                    IllegalArgumentException("Both servers need a name and a port"),
                )
            account = account.copy(
                imapHost = e.imapHost, imapPort = e.imapPort,
                smtpHost = e.smtpHost, smtpPort = e.smtpPort,
                login = login.trim(),
                provider = "custom",
            )
        }
        account.problem?.let { return Result.failure(IllegalArgumentException(it)) }
        return Result.success(account)
    }

    data class Endpoints(
        val imapHost: String,
        val imapPort: Int,
        val smtpHost: String,
        val smtpPort: Int,
    )
}
