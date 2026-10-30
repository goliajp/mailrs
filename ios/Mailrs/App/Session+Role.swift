import Foundation

extension Wire {
    /// What the signed-in account may do, as `GET /api/auth/me` says.
    ///
    /// Backend: `crates/webapi/src/handlers/auth/mod.rs` — `AuthMeResponse`.
    /// It decides which pages Settings offers; the server still refuses
    /// what the account may not call, whatever the app shows.
    struct Role: Decodable, Equatable {
        let isAdmin: Bool
        let isSuper: Bool

        enum CodingKeys: String, CodingKey {
            case isAdmin = "is_admin"
            case isSuper = "is_super"
        }
    }
}

extension MailrsClient {
    func role() async throws -> Wire.Role {
        try await getJSON("/api/auth/me")
    }
}

extension Session {
    /// Ask the server again what this account may do: on sign-in, when the
    /// app comes back to the front, and after any 403, so a right taken
    /// away hides its pages without a new sign-in. A failed read keeps
    /// what was known; the server refuses the pages anyway if the right is
    /// gone.
    func refreshRole() async {
        guard let client else { return }
        if let role = try? await client.role() { self.role = role }
    }

    /// Settings' operator pages: the ones that show other people's mail
    /// traffic to a super administrator, the directory to an administrator.
    var offersDirectory: Bool { role?.isAdmin == true }
    var offersMailTraffic: Bool { role?.isSuper == true }
}
