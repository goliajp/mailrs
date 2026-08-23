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
    }

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
