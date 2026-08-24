import Foundation
import Observation

/// The merged list's state: every connected mailbox in one place.
@Observable
@MainActor
final class MailboxesModel {
    private(set) var accounts: [MailAccount] = []
    /// How many rows the list is currently showing.
    ///
    /// The list used to hold **every** row this device has and sort
    /// them on each body evaluation. That is a read that grows with
    /// the mailbox on a screen where nothing else does, and it is the
    /// reason the per-account cap could not rise — see
    /// `MailboxApply.perAccount`. The store answers a window instead.
    private(set) var shown = MailboxesModel.page

    /// One page of rows, filtered, searched and ordered **in SQL**.
    private(set) var visible: [MailboxRow] = []

    /// Unread per account, over everything held rather than the window.
    private(set) var unreadPerAccount: [String: Int] = [:]

    /// Whether the store had more than the window asked for.
    private(set) var moreHeld = false

    /// How many rows a page is.
    ///
    /// Enough to fill several screens, so scrolling does not stop to
    /// think, and small enough that the first paint after launch is a
    /// window rather than a mailbox. Growing it is a `LIMIT` — it
    /// costs the same whether the device holds a hundred rows or a
    /// hundred thousand.
    static let page = 200
    /// Which accounts the list is limited to. Empty means **no
    /// filter** — the ordinary case, and the one a person gets without
    /// choosing anything.
    var only: Set<String> = [] { didSet { refresh() } }
    /// What is being searched for, if anything.
    var query = "" { didSet { refresh() } }
    private(set) var syncing = false
    /// What went wrong per account, last pass. An account missing from
    /// here is an account that worked, including one that fetched
    /// nothing.
    private(set) var failures: [String: String] = [:]

    func load() {
        accounts = AccountStore.load()
        // An account can be removed while this screen is not looking.
        // Its rows go with it (`AccountStore.remove`), but a filter
        // naming it would leave the list empty with no way back.
        only = only.filter { id in accounts.contains { $0.id == id } }
        reload()
    }

    /// Ask the store again.
    ///
    /// Called by every writer of the things it depends on — the filter
    /// and the query set it from their own `didSet`, and anything that
    /// changes the store calls it — rather than by callers remembering
    /// to. A derived value somebody has to remember to refresh is one
    /// that eventually disagrees with what it was derived from, and
    /// disagreeing here means showing the wrong mail.
    ///
    /// Private, so nothing outside can pick this when it meant
    /// `reload` and quietly leave the badges behind.
    private func refresh() {
        var filter: Set<String>?
        if !only.isEmpty { filter = only }
        let words = MailboxSearch.words(of: query)
        visible =
            words.isEmpty
            ? AccountStore.newest(shown, accounts: filter)
            : AccountStore.search(words, shown, accounts: filter)
        moreHeld = MailboxWindow.moreHeld(returned: visible.count, asked: shown)
    }

    /// The unread badges.
    ///
    /// **Apart from `refresh`**, and called only when the store
    /// changed: the badges count every unread row this device holds,
    /// which is a read over the whole table, and hanging it off the
    /// search box would run it once per keystroke to produce the same
    /// numbers every time.
    private func recount() {
        unreadPerAccount = AccountStore.unreadPerAccount()
    }

    /// The store changed: read the window again **and** recount.
    ///
    /// The two are apart because they have different reasons to run —
    /// a keystroke changes the window and not the counts — and
    /// together here because "the store changed" changes both. A
    /// caller that has to remember which one to use eventually
    /// forgets, so nothing outside this file has the choice.
    func reload() {
        refresh()
        recount()
    }

    /// Show more of what is already here.
    ///
    /// Before asking the server for older mail, which is a different
    /// and much slower thing — see `fetchEarlier`.
    func showMore() {
        guard moreHeld else { return }
        shown += MailboxesModel.page
        refresh()
    }

    /// How many unread messages are in the window.
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
            reload()
        }
        syncing = false
    }

    /// Move one message to its account's trash.
    ///
    /// The row goes when the **server** says it has: one that vanishes
    /// first and then fails to move comes back on the next fetch,
    /// looking like a bug rather than like the failure it was.
    func delete(_ row: MailboxRow) async {
        guard let account = account(for: row) else { return }
        switch await MailboxActions.delete(row, from: account) {
        case .done: reload()
        case let .failed(why): failures[account.id] = why
        }
    }

    func markUnread(_ row: MailboxRow) async {
        guard let account = account(for: row) else { return }
        switch await MailboxActions.markUnread(row, from: account) {
        case .done: reload()
        case let .failed(why): failures[account.id] = why
        }
    }

    /// Whether a reach for older mail is in flight.
    private(set) var reaching = false

    /// Fetch the mail before what is held.
    ///
    /// Every folder this device holds something of. A folder it has
    /// never fetched has no anchor to reach back from, and the
    /// ordinary pass is what gives it one.
    func fetchEarlier() async {
        guard !reaching else { return }
        reaching = true
        failures = [:]
        var targets = accounts
        if !only.isEmpty { targets = accounts.filter { only.contains($0.id) } }
        for account in targets {
            let folders = Set(AccountStore.folders(account.id))
            for folder in folders.sorted() {
                let outcome = await MailboxSyncRunner.earlier(account, folder: folder)
                if let why = outcome.failure { failures[account.id] = why }
                reload()
            }
        }
        reaching = false
    }
}
