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
        saveRows(MailboxApply.withoutAccount(rows(), id))
        var all = marks()
        let prefix = id + "/"
        for key in all.keys where key.hasPrefix(prefix) { all[key] = nil }
        saveMarks(all)
        UserDefaults.standard.removeObject(forKey: popSeenKey + id)
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
    static func rows() -> [MailboxRow] {
        guard let data = UserDefaults.standard.data(forKey: rowsForMailKey),
              let rows = try? JSONDecoder().decode([MailboxRow].self, from: data)
        else { return [] }
        return rows
    }

    static func saveRows(_ rows: [MailboxRow]) {
        guard let data = try? JSONEncoder().encode(rows) else { return }
        UserDefaults.standard.set(data, forKey: rowsForMailKey)
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
