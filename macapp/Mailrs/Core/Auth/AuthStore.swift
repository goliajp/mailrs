import Foundation
import Observation

@MainActor
@Observable
final class AuthStore {
    private(set) var authInfo: AuthInfo?
    private(set) var isRestoring: Bool = true

    private let keychain: KeychainTokenStore
    private let userDefaults: UserDefaults

    private static let metaKey = "mailrs_auth_meta"
    private static let rememberedEmailKey = "mailrs_last_address"

    var isAuthenticated: Bool { authInfo != nil }

    var rememberedEmail: String? {
        get { userDefaults.string(forKey: Self.rememberedEmailKey) }
        set { userDefaults.set(newValue, forKey: Self.rememberedEmailKey) }
    }

    init(keychain: KeychainTokenStore = KeychainTokenStore(), userDefaults: UserDefaults = .standard) {
        self.keychain = keychain
        self.userDefaults = userDefaults
    }

    func restore() async {
        defer { isRestoring = false }

        guard let token = keychain.load(),
              let data = userDefaults.data(forKey: Self.metaKey),
              let meta = try? JSONCoders.decoder.decode(AuthInfoMeta.self, from: data) else {
            authInfo = nil
            return
        }

        authInfo = AuthInfo(
            token: token,
            address: meta.address,
            display_name: meta.display_name,
            permissions: meta.permissions,
            accessible_domains: meta.accessible_domains,
            send_as: meta.send_as
        )
    }

    func persist(_ info: AuthInfo) throws {
        try keychain.save(info.token)
        let meta = AuthInfoMeta(
            address: info.address,
            display_name: info.display_name,
            permissions: info.permissions,
            accessible_domains: info.accessible_domains,
            send_as: info.send_as
        )
        let data = try JSONCoders.encoder.encode(meta)
        userDefaults.set(data, forKey: Self.metaKey)
        authInfo = info
        rememberedEmail = info.address
    }

    func signOut() {
        keychain.clear()
        userDefaults.removeObject(forKey: Self.metaKey)
        authInfo = nil
    }
}

extension AuthStore: TokenProvider {
    nonisolated func currentToken() async -> String? {
        await MainActor.run { self.authInfo?.token }
    }

    nonisolated func handleUnauthorized() async {
        await MainActor.run { self.signOut() }
    }
}

private struct AuthInfoMeta: Codable {
    let address: String
    let display_name: String?
    let permissions: [String]
    let accessible_domains: [String]
    let send_as: [String]?
}
