import Foundation
import Security

/// Where the session token lives between launches.
///
/// The Keychain, not `UserDefaults`: a token is a credential, and
/// `UserDefaults` is a plist inside the app container that goes into
/// unencrypted backups. `afterFirstUnlock` rather than `whenUnlocked` so
/// a background refresh on a locked phone can still use it.
enum TokenStore {
    private static let service = "jp.golia.mailrs"
    private static let account = "session-token"

    static func save(_ token: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
        var attributes = query
        attributes[kSecValueData as String] = Data(token.utf8)
        attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        SecItemAdd(attributes as CFDictionary, nil)
    }

    static func load() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data else { return nil }
        return String(decoding: data, as: UTF8.self)
    }

    /// The address is not a secret, but it lives beside the token so the
    /// two are cleared together — a stale address outliving its session
    /// is how a sign-in screen ends up prefilled with the wrong account.
    static func saveAddress(_ address: String) {
        UserDefaults.standard.set(address, forKey: "signed-in-address")
    }

    static func loadAddress() -> String? {
        UserDefaults.standard.string(forKey: "signed-in-address")
    }

    static func clear() {
        UserDefaults.standard.removeObject(forKey: "signed-in-address")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }
}
