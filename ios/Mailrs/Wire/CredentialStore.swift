import Foundation
import LocalAuthentication
import Security

/// The password, kept behind Face ID so signing in again is a look
/// rather than a typing exercise.
///
/// Separate from `TokenStore`, and deliberately outliving it: signing
/// out ends the *session*, and the point of this is that the next
/// sign-in does not need the keyboard. Clearing it is its own action.
///
/// `.userPresence`, not `.biometryCurrentSet`: the latter invalidates
/// the item when a face or a finger is added, and the symptom of that
/// is a credential that silently stops working with nothing on screen
/// to explain why. Presence accepts the device passcode too, which is
/// the same bar the phone's own lock screen sets.
enum CredentialStore {
    private static let service = "jp.golia.mailrs.credential"
    private static let lastAddressKey = "mailrs.lastAddress"

    /// The address of the last **successful** sign-in.
    ///
    /// Survives sign-out, unlike `TokenStore.loadAddress()`, which is
    /// scoped to the session and cleared with it. A sign-in form that
    /// has forgotten who you are on every launch is the one thing this
    /// screen should never do.
    static var lastAddress: String? {
        get { UserDefaults.standard.string(forKey: lastAddressKey) }
        set { UserDefaults.standard.set(newValue, forKey: lastAddressKey) }
    }

    /// Whether there is a stored password for `address`.
    ///
    /// Asked with `kSecUseAuthenticationUI: .fail`, so it answers
    /// without putting a Face ID sheet on screen — the button has to
    /// know whether to appear before anyone taps it.
    static func has(address: String) -> Bool {
        let context = LAContext()
        // Ask without prompting: the button has to know whether to
        // appear before anyone taps it. `interactionNotAllowed` makes a
        // present-but-locked item answer `errSecInteractionNotAllowed`,
        // which is a yes.
        context.interactionNotAllowed = true
        var query = base(address)
        query[kSecUseAuthenticationContext as String] = context
        query[kSecReturnData as String] = false
        let status = SecItemCopyMatching(query as CFDictionary, nil)
        return status == errSecSuccess || status == errSecInteractionNotAllowed
    }

    /// Store it, replacing whatever was there for this address.
    static func save(password: String, address: String) {
        remove(address: address)
        guard let access = SecAccessControlCreateWithFlags(
            nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, .userPresence, nil)
        else { return }
        var query = base(address)
        query[kSecValueData as String] = Data(password.utf8)
        query[kSecAttrAccessControl as String] = access
        SecItemAdd(query as CFDictionary, nil)
    }

    /// Read it, which puts the Face ID sheet on screen. `nil` when
    /// there is nothing stored or the reader did not get past it.
    static func password(for address: String, reason: String) -> String? {
        let context = LAContext()
        context.localizedReason = reason
        var query = base(address)
        query[kSecReturnData as String] = true
        query[kSecUseAuthenticationContext as String] = context
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data
        else { return nil }
        return String(decoding: data, as: UTF8.self)
    }

    /// Forget it — after the server refuses it, or when asked.
    static func remove(address: String) {
        SecItemDelete(base(address) as CFDictionary)
    }

    private static func base(_ address: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: address,
        ]
    }
}
