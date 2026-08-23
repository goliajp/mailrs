import Foundation
import Observation

/// The merged list's state: every connected mailbox in one place.
@Observable
@MainActor
final class MailboxesModel {
    private(set) var accounts: [MailAccount] = []
    private(set) var rows: [MailboxRow] = []
    /// Which accounts the list is limited to. Empty means **no
    /// filter** — the ordinary case, and the one a person gets without
    /// choosing anything.
    var only: Set<String> = []
    private(set) var syncing = false
    /// What went wrong per account, last pass. An account missing from
    /// here is an account that worked, including one that fetched
    /// nothing.
    private(set) var failures: [String: String] = [:]

    func load() {
        accounts = AccountStore.load()
        rows = AccountStore.rows()
        // An account can be removed while this screen is not looking.
        // Its rows go with it (`AccountStore.remove`), but a filter
        // naming it would leave the list empty with no way back.
        only = only.filter { id in accounts.contains { $0.id == id } }
    }

    /// The rows to show, filtered and newest first.
    var visible: [MailboxRow] {
        MailboxMerge.newestFirst(
            MailboxMerge.onlyAccounts(rows, only.isEmpty ? nil : only))
    }

    var unread: Int { MailboxMerge.unreadCount(visible) }

    /// The account a row came from, for its dot and its name.
    func account(for row: MailboxRow) -> MailAccount? {
        accounts.first { $0.id == row.accountId }
    }

    func toggle(_ accountId: String) {
        if only.contains(accountId) { only.remove(accountId) } else { only.insert(accountId) }
    }

    /// Fetch from every account, or from the ones being shown.
    ///
    /// Sequential rather than parallel: a phone on a train opening six
    /// TLS connections at once finishes no sooner and fails messier,
    /// and each account's rows land as it goes so the list fills in
    /// rather than waiting for the slowest server.
    func sync() async {
        guard !syncing else { return }
        syncing = true
        failures = [:]
        let targets = only.isEmpty ? accounts : accounts.filter { only.contains($0.id) }
        for account in targets {
            let outcome = await MailboxSyncRunner.run(account)
            if let failure = outcome.failure { failures[account.id] = failure }
            rows = AccountStore.rows()
        }
        syncing = false
    }
}
