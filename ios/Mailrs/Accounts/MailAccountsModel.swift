import Foundation
import Observation

/// The accounts screen's state.
@Observable
@MainActor
final class MailAccountsModel {
    private(set) var accounts: [MailAccount] = []
    /// What is being added, if anything.
    var draft = Draft()
    var busy = false
    var failure: String?

    struct Draft {
        var address = ""
        var secret = ""
        var name = ""
        /// Opened only when somebody asks: a form that starts with
        /// eight empty boxes teaches everybody that connecting mail is
        /// hard.
        var manual = false
        /// How mail is read, when the servers are typed by hand.
        ///
        /// Only offered there: a preset knows its own answer, and
        /// asking somebody to choose a protocol for Gmail is asking a
        /// question whose answer is already on file.
        var incoming: Incoming = .imap
        var imapHost = ""
        var imapPort = ""
        var smtpHost = ""
        var smtpPort = ""
        var login = ""

        /// A partial address is not a domain: looking one up for "s",
        /// "so", "som" is three answers nobody asked for.
        var addressLooksComplete: Bool {
            let parts = address.split(separator: "@")
            return parts.count == 2 && parts[1].contains(".")
        }

        /// What is known about this address, once it is an address.
        var provider: MailProvider? {
            addressLooksComplete ? MailProvider.forAddress(address) : nil
        }

        /// The provider's own word for what to type, or the plain one.
        var secretLabel: String { provider?.secretHelp?.what ?? "Password" }
    }

    func load() {
        accounts = AccountStore.load()
    }

    /// Add the account being drafted, after proving it works.
    func add() async {
        failure = nil
        guard draft.addressLooksComplete else {
            failure = "Enter the full email address of the account to add"
            return
        }
        var account = MailAccount.make(
            address: draft.address.trimmingCharacters(in: .whitespaces),
            displayName: draft.name.trimmingCharacters(in: .whitespaces),
            sort: accounts.count)
        if draft.manual {
            guard let manual = manualEndpoints() else {
                failure = "Both servers need a name and a port"
                return
            }
            account.imapHost = manual.imapHost
            account.imapPort = manual.imapPort
            account.smtpHost = manual.smtpHost
            account.smtpPort = manual.smtpPort
            account.login = draft.login.trimmingCharacters(in: .whitespaces)
            account.incoming = draft.incoming
            account.provider = "custom"
        }
        if let problem = account.problem {
            failure = problem
            return
        }
        busy = true
        defer { busy = false }
        // Proved before it is stored: a credential saved and then found
        // to be wrong is an account that sits in the list doing nothing,
        // and that is indistinguishable from having no new mail.
        if let bad = await AccountConnection.verify(account, secret: draft.secret) {
            failure = "The \(bad.stage.rawValue) refused: \(bad.message)"
            return
        }
        AccountStore.saveSecret(draft.secret, for: account.id)
        AccountStore.upsert(account)
        draft = Draft()
        load()
    }

    func remove(_ account: MailAccount) {
        AccountStore.remove(id: account.id)
        load()
    }

    /// The two endpoints as typed, or nothing.
    ///
    /// Digits only for a port, deliberately: `UInt16(" 993")` is nil
    /// but `UInt16("+993")` is nil too on some paths and 993 on
    /// others, and neither is what somebody typing a port means.
    /// Surrounding spaces are trimmed — a paste is not a mistake.
    func manualEndpoints() -> (imapHost: String, imapPort: UInt16, smtpHost: String,
                               smtpPort: UInt16)? {
        func port(_ s: String) -> UInt16? {
            let t = s.trimmingCharacters(in: .whitespaces)
            guard !t.isEmpty, t.allSatisfy(\.isNumber), let n = UInt16(t), n >= 1 else {
                return nil
            }
            return n
        }
        let ih = draft.imapHost.trimmingCharacters(in: .whitespaces)
        let sh = draft.smtpHost.trimmingCharacters(in: .whitespaces)
        guard !ih.isEmpty, let ip = port(draft.imapPort) else { return nil }
        // JMAP has one endpoint and no separate outgoing server — it
        // submits over the same API. Demanding an SMTP host there is
        // asking for something that does not exist, and somebody will
        // type the incoming one again to get past the form.
        if draft.incoming == .jmap { return (ih, ip, "", 0) }
        guard !sh.isEmpty, let sp = port(draft.smtpPort) else { return nil }
        return (ih, ip, sh, sp)
    }
}
