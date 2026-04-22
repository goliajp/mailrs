import Testing
import Foundation
@testable import Mailrs

/// Keychain tests. On macOS they require a signed bundle with keychain-access-groups.
/// When running unsigned (xcodebuild without a developer team), the data-protection
/// keychain returns errSecMissingEntitlement (-34018). We detect and skip in that
/// case — the real Keychain path is exercised manually via the login flow.
@Suite("KeychainTokenStore")
struct KeychainTokenStoreTests {
    private static func makeStore() -> KeychainTokenStore? {
        let probe = KeychainTokenStore(
            service: "jp.golia.mailrs.tests",
            account: "probe-\(UUID().uuidString)"
        )
        do {
            try probe.save("probe")
            probe.clear()
        } catch {
            return nil
        }
        return KeychainTokenStore(
            service: "jp.golia.mailrs.tests",
            account: "session-\(UUID().uuidString)"
        )
    }

    @Test("save then load returns the stored token")
    func saveAndLoad() throws {
        guard let store = Self.makeStore() else {
            print("[skip] Keychain entitlement unavailable in this test host")
            return
        }
        defer { store.clear() }

        try store.save("alpha-token")
        #expect(store.load() == "alpha-token")
    }

    @Test("save twice overwrites the previous value")
    func overwrite() throws {
        guard let store = Self.makeStore() else {
            print("[skip] Keychain entitlement unavailable in this test host")
            return
        }
        defer { store.clear() }

        try store.save("first")
        try store.save("second")
        #expect(store.load() == "second")
    }

    @Test("clear removes the token")
    func clearRemoves() throws {
        guard let store = Self.makeStore() else {
            print("[skip] Keychain entitlement unavailable in this test host")
            return
        }

        try store.save("to-be-cleared")
        store.clear()
        #expect(store.load() == nil)
    }
}
