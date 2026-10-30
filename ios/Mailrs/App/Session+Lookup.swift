import Foundation

extension Session {
    /// A sender domain's brand icon, or nil when there is none.
    func icon(domain: String) async -> Data? {
        guard let client else { return nil }
        return try? await client.icon(domain: domain)
    }


    /// Contact suggestions for a To field. Failures answer as no
    /// suggestions — autocomplete is an offer, not a feature that may
    /// interrupt composing with an error.
    func contacts(matching query: String) async -> [String] {
        guard let client else { return [] }
        return (try? await client.contacts(matching: query)) ?? []
    }


    func attachment(uid: UInt32, index: Int) async throws -> Data {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.attachment(uid: uid, index: index)
    }


    /// Upload this device's APNs token, so the server can reach it.
    func registerPushToken(_ token: String) async {
        guard let client else { return }
        try? await client.registerPushToken(token)
    }
}
