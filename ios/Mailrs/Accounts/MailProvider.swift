import Foundation

/// Where a mail provider's servers are, and what it calls the secret.
///
/// The table exists so adding an account is an address and a password
/// rather than eight fields. What it cannot guess is the secret — and
/// for half of these the thing to type is not the login password at
/// all but a code generated in the provider's own web UI.
///
/// Typing a login password into a field labelled 授权码 is a mistake
/// somebody recovers from. Typing it into one labelled "Password" and
/// being refused with `LOGIN failed` is not.
struct MailProvider: Equatable, Sendable {
    /// What to show a person: `Gmail`, not `gmail.com`.
    let label: String
    let imapHost: String
    let imapPort: UInt16
    let smtpHost: String
    let smtpPort: UInt16
    /// What the provider will accept.
    let auth: AuthKind
    /// The provider's own word for the secret, and where to get one.
    let secretHelp: SecretHelp?
    /// Folders to leave alone.
    ///
    /// Gmail's All Mail holds a copy of everything, so reading it
    /// doubles every message; Trash and Spam are the two a person
    /// would skip themselves.
    let skipFolders: [String]

    enum AuthKind: String, Codable, Equatable, Sendable {
        /// The ordinary login password.
        case password
        /// A code generated in the provider's web UI.
        case appPassword = "app_password"
        /// The provider refuses passwords for mail clients entirely.
        case oauth2
    }

    struct SecretHelp: Equatable, Sendable {
        /// The provider's own word: "app password", "授权码".
        let what: String
        /// Where to make one.
        let url: String
    }
}

extension MailProvider {
    /// The provider for an address, by its domain.
    static func forAddress(_ address: String) -> MailProvider? {
        guard let at = address.lastIndex(of: "@") else { return nil }
        return forDomain(String(address[address.index(after: at)...]))
    }

    /// The provider for a domain, or nil if it is not one of these.
    ///
    /// Matched on the whole domain, not a suffix: `notgmail.com` is
    /// not Gmail, and a suffix match would send somebody's password to
    /// the wrong server.
    static func forDomain(_ domain: String) -> MailProvider? {
        table[domain.lowercased()]
    }

    /// The providers this app knows without asking anybody.
    ///
    /// Ordered by who people actually have, not alphabetically. The
    /// aliases are the ones a person is likely to type: `googlemail.com`
    /// is Gmail, `hotmail.co.jp` is Outlook.
    static let table: [String: MailProvider] = {
        let gmail = MailProvider(
            label: "Gmail",
            imapHost: "imap.gmail.com", imapPort: 993,
            smtpHost: "smtp.gmail.com", smtpPort: 465,
            auth: .oauth2,
            secretHelp: nil,
            // A copy of every message lives here; reading it doubles
            // the mailbox.
            skipFolders: ["[Gmail]/All Mail", "[Gmail]/Trash", "[Gmail]/Spam"]
        )
        let outlook = MailProvider(
            label: "Outlook",
            imapHost: "outlook.office365.com", imapPort: 993,
            smtpHost: "smtp.office365.com", smtpPort: 587,
            auth: .oauth2,
            secretHelp: nil,
            skipFolders: ["Deleted Items", "Junk Email"]
        )
        let qq = MailProvider(
            label: "QQ",
            imapHost: "imap.qq.com", imapPort: 993,
            smtpHost: "smtp.qq.com", smtpPort: 465,
            auth: .appPassword,
            secretHelp: .init(what: "授权码", url: "https://service.mail.qq.com/detail/0/75"),
            skipFolders: ["已删除", "垃圾邮件"]
        )
        let netease163 = MailProvider(
            label: "网易 163",
            imapHost: "imap.163.com", imapPort: 993,
            smtpHost: "smtp.163.com", smtpPort: 465,
            auth: .appPassword,
            secretHelp: .init(what: "授权码", url: "https://mail.163.com/"),
            skipFolders: ["已删除", "垃圾邮件"]
        )
        let yahooJP = MailProvider(
            label: "Yahoo! JAPAN",
            imapHost: "imap.mail.yahoo.co.jp", imapPort: 993,
            smtpHost: "smtp.mail.yahoo.co.jp", smtpPort: 465,
            auth: .appPassword,
            secretHelp: .init(what: "アプリパスワード",
                              url: "https://support.yahoo-net.jp/PccMail/s/article/H000007321"),
            skipFolders: ["ゴミ箱", "迷惑メール"]
        )
        let icloud = MailProvider(
            label: "iCloud",
            imapHost: "imap.mail.me.com", imapPort: 993,
            smtpHost: "smtp.mail.me.com", smtpPort: 587,
            auth: .appPassword,
            secretHelp: .init(what: "app-specific password",
                              url: "https://support.apple.com/en-us/102654"),
            skipFolders: ["Deleted Messages", "Junk"]
        )
        let fastmail = MailProvider(
            label: "Fastmail",
            imapHost: "imap.fastmail.com", imapPort: 993,
            smtpHost: "smtp.fastmail.com", smtpPort: 465,
            auth: .appPassword,
            secretHelp: .init(what: "app password",
                              url: "https://app.fastmail.com/settings/security/apps"),
            skipFolders: ["Trash", "Spam"]
        )
        var t: [String: MailProvider] = [:]
        for d in ["gmail.com", "googlemail.com"] { t[d] = gmail }
        for d in ["outlook.com", "hotmail.com", "hotmail.co.jp", "live.com", "msn.com"] {
            t[d] = outlook
        }
        t["qq.com"] = qq
        t["foxmail.com"] = qq
        for d in ["163.com", "126.com"] { t[d] = netease163 }
        for d in ["yahoo.co.jp", "ymail.ne.jp"] { t[d] = yahooJP }
        for d in ["icloud.com", "me.com", "mac.com"] { t[d] = icloud }
        for d in ["fastmail.com", "fastmail.fm"] { t[d] = fastmail }
        return t
    }()

    /// A starting point for a domain nobody knows.
    ///
    /// The convention almost every small host follows, and the one a
    /// person would try first. Offered as something to correct rather
    /// than as a promise — the set-up screen shows it filled in.
    static func guess(forDomain domain: String) -> MailProvider {
        MailProvider(
            label: domain,
            imapHost: "imap.\(domain)", imapPort: 993,
            smtpHost: "smtp.\(domain)", smtpPort: 465,
            auth: .password,
            secretHelp: nil,
            skipFolders: ["Trash", "Junk", "Spam"]
        )
    }
}
