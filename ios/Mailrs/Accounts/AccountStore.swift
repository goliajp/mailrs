import Foundation
import Security

/// The accounts this person has added, and their secrets.
///
/// Two stores, deliberately apart. The **rows** are ordinary
/// preferences — they hold no secret and a person may want to see
/// them. The **credentials** are keychain items, one per account,
/// which is what makes "delete the account" also mean "the password
/// is gone" rather than leaving a secret nobody can see and nobody
/// will remove.
enum AccountStore {
    private static let rowsKey = "mailrs.accounts.v1"
    private static let service = "jp.golia.mailrs.account"

    // MARK: - rows

    /// Forget every account when the run asks to start empty.
    ///
    /// `-mailrsFreshCache` wiped the mail cache and left the accounts,
    /// so a mailbox added by one test was still connected in the next
    /// **run** — and a test asserting "no mailboxes yet" passed only
    /// because the alphabet happened to put it before the test that
    /// adds one. Run on its own, or run twice, it failed and read as a
    /// screen that had stopped opening.
    ///
    /// The rows only; the keychain items are cleaned up by `remove`,
    /// and a boot flag should not reach into the keychain.
    static func bootstrapIfAskedToStartEmpty() {
        guard ProcessInfo.processInfo.arguments.contains("-mailrsFreshCache")
        else { return }
        UserDefaults.standard.removeObject(forKey: rowsKey)
    }

    static func load() -> [MailAccount] {
        guard let data = UserDefaults.standard.data(forKey: rowsKey),
              let rows = try? JSONDecoder().decode([MailAccount].self, from: data)
        else { return [] }
        return rows.sorted { $0.sort < $1.sort }
    }

    static func save(_ rows: [MailAccount]) {
        guard let data = try? JSONEncoder().encode(rows) else { return }
        UserDefaults.standard.set(data, forKey: rowsKey)
    }

    /// Add or replace one, keeping the list in order.
    static func upsert(_ account: MailAccount) {
        var rows = load().filter { $0.id != account.id }
        rows.append(account)
        save(rows)
    }

    /// Remove one **and its secret**.
    ///
    /// Both, always: a row removed while its keychain item stays
    /// behind is a credential nobody can see and nobody will delete.
    static func remove(id: String) {
        save(load().filter { $0.id != id })
        deleteSecret(for: id)
        // And its mail, and where each of its folders was left. A row
        // left behind is mail nobody can open — the credential and the
        // server it came from are both gone — and a mark left behind
        // makes the next account with the same address resume from
        // somebody else's place.
        // And the connection that credential signed in: a socket
        // left open is still authenticated as somebody who has just
        // been removed, and the next tap would reuse it.
        Task { await ImapPool.shared.drop(id) }
        migrateRowsOnce()
        try? database?.deleteAccount(id)
        var all = marks()
        let prefix = id + "/"
        for key in all.keys where key.hasPrefix(prefix) { all[key] = nil }
        saveMarks(all)
        UserDefaults.standard.removeObject(forKey: popSeenKey + id)
        UserDefaults.standard.removeObject(forKey: lastSyncKey + id)
    }

    // MARK: - the mail itself

    private static let rowsForMailKey = "mailrs.mailbox.rows.v1"
    private static let marksKey = "mailrs.mailbox.marks.v1"

    /// Every row from every connected mailbox.
    ///
    /// Ordinary preferences, not the keychain: these are headers, and
    /// a person can already see them on screen. The **bodies** are not
    /// stored at all — they are fetched when a message is opened, so
    /// nothing here grows without bound and nothing here is worth
    /// stealing.
    /// The table, opened once.
    ///
    /// `nonisolated(unsafe)` because SQLite is doing the serialising:
    /// the handle is opened `FULLMUTEX`, so concurrent use is safe at
    /// the layer that actually owns it, and wrapping it in an actor
    /// here would only move the same lock somewhere less honest.
    ///
    /// A database that cannot be opened leaves this nil and every
    /// accessor a no-op — the rows are a cache of what a server has,
    /// so the cost is a list that refills on the next pass rather than
    /// an app that will not start.
    nonisolated(unsafe) private static let database: MailboxDatabase? = {
        let directory = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first
        guard let directory else { return nil }
        try? FileManager.default.createDirectory(
            at: directory, withIntermediateDirectories: true
        )
        return try? MailboxDatabase(
            path: directory.appendingPathComponent("mailboxes.sqlite").path
        )
    }()

    static func rows() -> [MailboxRow] {
        migrateRowsOnce()
        return (try? database?.all()) .flatMap { $0 } ?? []
    }

    /// Throw away every row and keep these instead.
    ///
    /// Named for what it does. It is what a test that wants a known
    /// starting point needs, and what nothing on a sync path should
    /// use — `upsertRows`, `deleteRow` and `setRowSeen` address the
    /// rows that actually changed, which is the whole reason the rows
    /// moved out of one preferences blob.
    static func replaceRows(_ rows: [MailboxRow]) {
        migrateRowsOnce()
        try? database?.replaceAll(rows)
    }

    /// Add or update, leaving every other row alone.
    static func upsertRows(_ rows: [MailboxRow]) {
        migrateRowsOnce()
        try? database?.upsert(rows)
    }

    static func deleteRow(_ row: MailboxRow) {
        migrateRowsOnce()
        try? database?.delete(account: row.accountId, folder: row.folder, uid: row.uid)
    }

    static func deleteUids(_ accountId: String, _ folder: String, _ uids: [UInt32]) {
        migrateRowsOnce()
        try? database?.delete(account: accountId, folder: folder, uids: uids)
    }

    static func setRowSeen(_ row: MailboxRow, _ seen: Bool) {
        migrateRowsOnce()
        try? database?.setSeen(
            account: row.accountId, folder: row.folder, uid: row.uid, seen: seen
        )
    }

    static func setUidsSeen(_ accountId: String, _ folder: String, _ flags: [UInt32: Bool]) {
        migrateRowsOnce()
        try? database?.setSeen(account: accountId, folder: folder, flags: flags)
    }

    static func dropFolder(_ accountId: String, _ folder: String) {
        migrateRowsOnce()
        try? database?.deleteFolder(account: accountId, folder: folder)
    }

    /// The newest rows, in the order the list shows them.
    static func newest(_ limit: Int, accounts: Set<String>? = nil) -> [MailboxRow] {
        migrateRowsOnce()
        return (try? database?.newest(limit: limit, accounts: accounts)).flatMap { $0 } ?? []
    }

    /// The newest rows matching every word.
    static func search(_ words: [String], _ limit: Int, accounts: Set<String>? = nil)
        -> [MailboxRow]
    {
        migrateRowsOnce()
        return (try? database?.search(words: words, limit: limit, accounts: accounts))
            .flatMap { $0 } ?? []
    }

    /// Unread per account, over everything held rather than a window.
    static func unreadPerAccount() -> [String: Int] {
        migrateRowsOnce()
        return (try? database?.unreadPerAccount()).flatMap { $0 } ?? [:]
    }

    /// How many rows one account holds.
    ///
    /// A `COUNT(*)`, not a filter over every row: the one caller is the
    /// ceiling check on the "load earlier" path, and loading the table
    /// to decide whether the table is full is the read this whole layer
    /// exists to remove.
    static func count(_ accountId: String) -> Int {
        migrateRowsOnce()
        return (try? database?.count(account: accountId)).flatMap { $0 } ?? 0
    }

    /// Every folder this device holds something of, for one account.
    static func folders(_ accountId: String) -> [String] {
        migrateRowsOnce()
        return (try? database?.folders(account: accountId)).flatMap { $0 } ?? []
    }

    static func capAccount(_ accountId: String, limit: Int = MailboxApply.perAccount) {
        migrateRowsOnce()
        try? database?.cap(account: accountId, limit: limit)
    }

    /// Move whatever the preferences blob still holds into the table.
    ///
    /// Runs at most once per install: a device upgrading from a build
    /// that kept its rows as one JSON blob would otherwise show an
    /// empty list until the next sync, which reads as lost mail. The
    /// key is removed afterwards so a later downgrade-and-upgrade
    /// cannot resurrect rows the person has since deleted.
    private static func migrateRowsOnce() {
        guard let data = UserDefaults.standard.data(forKey: rowsForMailKey) else { return }
        let carried = (try? JSONDecoder().decode([MailboxRow].self, from: data)) ?? []
        try? database?.upsert(carried)
        UserDefaults.standard.removeObject(forKey: rowsForMailKey)
    }

    /// Where each folder of each account was left.
    ///
    /// Keyed `accountId/folder`, so two accounts with an INBOX each
    /// keep their own place — the mistake this key shape prevents is
    /// the same one `MailboxRow.id` prevents in the list.
    static func marks() -> [String: FolderMark] {
        guard let data = UserDefaults.standard.data(forKey: marksKey),
              let marks = try? JSONDecoder().decode([String: FolderMark].self, from: data)
        else { return [:] }
        return marks
    }

    static func saveMarks(_ marks: [String: FolderMark]) {
        guard let data = try? JSONEncoder().encode(marks) else { return }
        UserDefaults.standard.set(data, forKey: marksKey)
    }

    /// The marks of one account, without its prefix.
    static func marks(for accountId: String) -> [String: FolderMark] {
        var out: [String: FolderMark] = [:]
        let prefix = accountId + "/"
        for (key, mark) in marks() where key.hasPrefix(prefix) {
            out[String(key.dropFirst(prefix.count))] = mark
        }
        return out
    }

    /// Store one account's marks back, leaving every other account's
    /// alone.
    static func saveMarks(_ folderMarks: [String: FolderMark], for accountId: String) {
        var all = marks()
        let prefix = accountId + "/"
        for key in all.keys where key.hasPrefix(prefix) { all[key] = nil }
        for (folder, mark) in folderMarks { all[prefix + folder] = mark }
        saveMarks(all)
    }

    /// The uidls a POP3 account has already read.
    ///
    /// Not `FolderMark`: POP3 has no folders and no uid validity, and
    /// its message numbers are renumbered every session. The uidl is
    /// the only durable identity it offers, so what is remembered is a
    /// set of them — pruned each pass to what the server still has, or
    /// the bookkeeping outgrows the mailbox.
    static func popSeen(_ accountId: String) -> Set<String> {
        guard let data = UserDefaults.standard.data(forKey: popSeenKey + accountId),
            let ids = try? JSONDecoder().decode([String].self, from: data)
        else { return [] }
        return Set(ids)
    }

    static func savePopSeen(_ accountId: String, _ ids: Set<String>) {
        guard let data = try? JSONEncoder().encode(Array(ids)) else { return }
        UserDefaults.standard.set(data, forKey: popSeenKey + accountId)
    }

    private static let popSeenKey = "mailrs.pop.seen.v1."

    /// When each account was last read successfully.
    ///
    /// Kept so the list can say how old what it is showing is. "No new
    /// mail" and "we have not managed to check since yesterday" look
    /// identical on screen, and only one of them is a reason to relax.
    static func lastSync(_ accountId: String) -> Int64? {
        let at = UserDefaults.standard.object(forKey: lastSyncKey + accountId) as? NSNumber
        return at?.int64Value
    }

    static func saveLastSync(_ accountId: String, _ epochSeconds: Int64) {
        UserDefaults.standard.set(NSNumber(value: epochSeconds), forKey: lastSyncKey + accountId)
    }

    private static let lastSyncKey = "mailrs.last.sync.v1."

    // MARK: - secrets

    static func saveSecret(_ secret: String, for id: String) {
        deleteSecret(for: id)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: id,
            kSecValueData as String: Data(secret.utf8),
            // Available after the first unlock, and never synced to
            // another device: a mail password is this phone's.
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        SecItemAdd(query as CFDictionary, nil)
    }

    static func secret(for id: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: id,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var out: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess,
              let data = out as? Data
        else { return nil }
        return String(decoding: data, as: UTF8.self)
    }

    static func deleteSecret(for id: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: id,
        ]
        SecItemDelete(query as CFDictionary)
    }
}
