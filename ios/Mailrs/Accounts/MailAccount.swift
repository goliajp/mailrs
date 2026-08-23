import Foundation

/// A mailbox somewhere else, as this app holds it.
///
/// The credential is **not** here: it lives in the keychain under the
/// account's id, so a row can be logged, encoded, or shown on screen
/// without carrying a password through code that has no business
/// holding one.
/// How mail is read.
///
/// Not a preference: it changes what a mailbox *is*. POP3 has one
/// folder, no server-side flags and no stable message numbers, so read
/// state can only be kept on this device and anything filed elsewhere
/// is invisible. Saying so in the type keeps every reader honest about
/// it.
enum Incoming: String, Codable, Equatable, Sendable {
    case imap
    case pop3
    case jmap
}

struct MailAccount: Codable, Equatable, Identifiable, Sendable {
    let id: String
    /// The address, and what the server knows this person as unless
    /// `login` says otherwise.
    var address: String
    /// What to call it on screen. Empty means show the address.
    var displayName: String
    /// A preset id — `gmail`, `qq` — or `custom`.
    var provider: String
    var imapHost: String
    var imapPort: UInt16
    var smtpHost: String
    var smtpPort: UInt16
    /// The login name, when the server wants something other than the
    /// address. Empty means the address.
    var login: String
    /// How mail is read from this account.
    ///
    /// Defaulted, so a row written before this field existed still
    /// decodes — and defaulted to IMAP, which is what every account
    /// written before it existed was.
    var incoming: Incoming = .imap
    var auth: MailProvider.AuthKind
    /// Folders this account does not read.
    var skipFolders: [String]
    /// Where it sits in the list.
    var sort: Int

    /// What the server is told to call this person.
    var loginName: String { login.isEmpty ? address : login }
    /// What a person sees.
    var title: String { displayName.isEmpty ? address : displayName }

    /// A stable id for an address.
    ///
    /// Derived rather than random so adding the same account twice is
    /// the same row rather than two — and so a keychain item survives
    /// a list rebuilt from scratch.
    static func id(for address: String) -> String {
        "acct-" + address.lowercased().unicodeScalars.reduce(into: UInt64(0xcbf2_9ce4_8422_2325)) {
            h, c in
            h ^= UInt64(c.value)
            h = h &* 0x100_0000_01b3
        }.description
    }

    /// A row filled in from what is known about the address.
    static func make(address: String, displayName: String = "", sort: Int = 0) -> MailAccount {
        let known = MailProvider.forAddress(address)
        let domain = address.split(separator: "@").last.map(String.init) ?? ""
        let p = known ?? MailProvider.guess(forDomain: domain)
        return MailAccount(
            id: id(for: address),
            address: address,
            displayName: displayName,
            provider: known == nil ? "custom" : p.label.lowercased(),
            imapHost: p.imapHost, imapPort: p.imapPort,
            smtpHost: p.smtpHost, smtpPort: p.smtpPort,
            login: "",
            auth: p.auth,
            skipFolders: p.skipFolders,
            sort: sort)
    }

    /// What is wrong with this account, in words a person can act on.
    ///
    /// Checked here rather than at the server so a set-up screen can
    /// say what is missing before spending thirty seconds finding out
    /// that a blank host does not resolve.
    var problem: String? {
        if !address.contains("@") || address.hasPrefix("@") || address.hasSuffix("@") {
            return "That is not an email address"
        }
        if imapHost.trimmingCharacters(in: .whitespaces).isEmpty {
            return "The incoming server needs a name"
        }
        if imapPort == 0 { return "A port must be between 1 and 65535" }
        // JMAP submits over the same API it reads from, so there is no
        // second server and demanding one refuses an account that is
        // perfectly well described. Every other kind has both.
        if incoming == .jmap { return nil }
        if smtpHost.trimmingCharacters(in: .whitespaces).isEmpty {
            return "The outgoing server needs a name"
        }
        if smtpPort == 0 { return "A port must be between 1 and 65535" }
        return nil
    }
}
