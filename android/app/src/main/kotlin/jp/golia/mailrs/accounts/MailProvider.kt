package jp.golia.mailrs.accounts

/**
 * Where a mail provider's servers are, and what it calls the secret.
 *
 * The table exists so adding an account is an address and a password
 * rather than eight fields. What it cannot guess is the secret — and
 * for half of these the thing to type is not the login password at all
 * but a code generated in the provider's own web UI.
 *
 * Typing a login password into a field labelled 授权码 is a mistake
 * somebody recovers from. Typing it into one labelled "Password" and
 * being refused with `LOGIN failed` is not.
 */
data class MailProvider(
    /** What to show a person: `Gmail`, not `gmail.com`. */
    val label: String,
    val imapHost: String,
    val imapPort: Int,
    val smtpHost: String,
    val smtpPort: Int,
    val auth: AuthKind,
    /** The provider's own word for the secret, and where to get one. */
    val secretHelp: SecretHelp?,
    /**
     * Folders to leave alone.
     *
     * Gmail's All Mail holds a copy of everything, so reading it
     * doubles every message; Trash and Spam are the two a person would
     * skip themselves.
     */
    val skipFolders: List<String>,
) {
    enum class AuthKind(val wire: String) {
        /** The ordinary login password. */
        PASSWORD("password"),

        /** A code generated in the provider's web UI. */
        APP_PASSWORD("app_password"),

        /** The provider refuses passwords for mail clients entirely. */
        OAUTH2("oauth2"),
    }

    data class SecretHelp(val what: String, val url: String)

    companion object {
        /** The provider for an address, by its domain. */
        fun forAddress(address: String): MailProvider? {
            val at = address.lastIndexOf('@')
            if (at < 0) return null
            return forDomain(address.substring(at + 1))
        }

        /**
         * The provider for a domain, or null if it is not one of these.
         *
         * Matched on the whole domain, not a suffix: `notgmail.com` is
         * not Gmail, and a suffix match would send somebody's password
         * to the wrong server.
         */
        fun forDomain(domain: String): MailProvider? = table[domain.lowercase()]

        private val gmail = MailProvider(
            label = "Gmail",
            imapHost = "imap.gmail.com", imapPort = 993,
            smtpHost = "smtp.gmail.com", smtpPort = 465,
            auth = AuthKind.OAUTH2,
            secretHelp = null,
            skipFolders = listOf("[Gmail]/All Mail", "[Gmail]/Trash", "[Gmail]/Spam"),
        )
        private val outlook = MailProvider(
            label = "Outlook",
            imapHost = "outlook.office365.com", imapPort = 993,
            smtpHost = "smtp.office365.com", smtpPort = 587,
            auth = AuthKind.OAUTH2,
            secretHelp = null,
            skipFolders = listOf("Deleted Items", "Junk Email"),
        )
        private val qq = MailProvider(
            label = "QQ",
            imapHost = "imap.qq.com", imapPort = 993,
            smtpHost = "smtp.qq.com", smtpPort = 465,
            auth = AuthKind.APP_PASSWORD,
            secretHelp = SecretHelp("授权码", "https://service.mail.qq.com/detail/0/75"),
            skipFolders = listOf("已删除", "垃圾邮件"),
        )
        private val netease = MailProvider(
            label = "网易 163",
            imapHost = "imap.163.com", imapPort = 993,
            smtpHost = "smtp.163.com", smtpPort = 465,
            auth = AuthKind.APP_PASSWORD,
            secretHelp = SecretHelp("授权码", "https://mail.163.com/"),
            skipFolders = listOf("已删除", "垃圾邮件"),
        )
        private val yahooJp = MailProvider(
            label = "Yahoo! JAPAN",
            imapHost = "imap.mail.yahoo.co.jp", imapPort = 993,
            smtpHost = "smtp.mail.yahoo.co.jp", smtpPort = 465,
            auth = AuthKind.APP_PASSWORD,
            secretHelp = SecretHelp(
                "アプリパスワード",
                "https://support.yahoo-net.jp/PccMail/s/article/H000007321",
            ),
            skipFolders = listOf("ゴミ箱", "迷惑メール"),
        )
        private val icloud = MailProvider(
            label = "iCloud",
            imapHost = "imap.mail.me.com", imapPort = 993,
            smtpHost = "smtp.mail.me.com", smtpPort = 587,
            auth = AuthKind.APP_PASSWORD,
            secretHelp = SecretHelp(
                "app-specific password",
                "https://support.apple.com/en-us/102654",
            ),
            skipFolders = listOf("Deleted Messages", "Junk"),
        )
        private val fastmail = MailProvider(
            label = "Fastmail",
            imapHost = "imap.fastmail.com", imapPort = 993,
            smtpHost = "smtp.fastmail.com", smtpPort = 465,
            auth = AuthKind.APP_PASSWORD,
            secretHelp = SecretHelp(
                "app password",
                "https://app.fastmail.com/settings/security/apps",
            ),
            skipFolders = listOf("Trash", "Spam"),
        )

        /**
         * The providers this app knows without asking anybody.
         *
         * The aliases are the ones a person is likely to type:
         * `googlemail.com` is Gmail, `hotmail.co.jp` is Outlook.
         */
        val table: Map<String, MailProvider> = buildMap {
            for (d in listOf("gmail.com", "googlemail.com")) put(d, gmail)
            for (d in listOf("outlook.com", "hotmail.com", "hotmail.co.jp", "live.com", "msn.com")) {
                put(d, outlook)
            }
            put("qq.com", qq)
            put("foxmail.com", qq)
            for (d in listOf("163.com", "126.com")) put(d, netease)
            for (d in listOf("yahoo.co.jp", "ymail.ne.jp")) put(d, yahooJp)
            for (d in listOf("icloud.com", "me.com", "mac.com")) put(d, icloud)
            for (d in listOf("fastmail.com", "fastmail.fm")) put(d, fastmail)
        }

        /**
         * A starting point for a domain nobody knows.
         *
         * The convention almost every small host follows, and the one a
         * person would try first. Offered as something to correct
         * rather than as a promise — the set-up screen shows it filled
         * in.
         */
        fun guess(domain: String) = MailProvider(
            label = domain,
            imapHost = "imap.$domain", imapPort = 993,
            smtpHost = "smtp.$domain", smtpPort = 465,
            auth = AuthKind.PASSWORD,
            secretHelp = null,
            skipFolders = listOf("Trash", "Junk", "Spam"),
        )
    }
}
