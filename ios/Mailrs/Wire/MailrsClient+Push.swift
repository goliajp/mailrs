import Foundation

/// This phone's push token, and ending its session.
extension MailrsClient {
    /// `POST /api/push/tokens` — hand the server this device's address.
    ///
    /// Backend: `crates/webapi/src/handlers/push.rs` —
    /// `RegisterPushTokenRequest`, pinned by
    /// `wire-contract/requests/push-register.json`.
    ///
    /// `resume` says the reader turned pushes on here just now: only then
    /// does the server take back a phone a person removed from another
    /// device, which it otherwise answers `410 {"error":"removed"}`.
    func registerPushToken(_ token: String, resume: Bool) async throws -> PushTokenOutcome {
        let body = try JSONEncoder().encode(PushTokenBody(token: token, platform: "ios", resume: resume))
        let (data, response) = try await send("POST", "/api/push/tokens", body: body, authorized: true)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        if PushTokenOutcome.isRemoved(status: status, body: data) { return .removed }
        guard (200..<300).contains(status) else { throw MailrsError.server(status: status) }
        return .registered
    }

    /// `POST /api/auth/logout` — end this session on the server too, which
    /// also stops pushes to this phone.
    func logout() async throws {
        try await verb("POST", "/api/auth/logout")
    }
}

struct PushTokenBody: Encodable {
    let token: String
    let platform: String
    let resume: Bool
}

/// What the server did with this phone's token.
enum PushTokenOutcome: Equatable {
    case registered
    /// A person removed this phone from another device.
    case removed

    static func isRemoved(status: Int, body: Data) -> Bool {
        struct Refusal: Decodable { let error: String }
        guard status == 410 else { return false }
        return (try? JSONDecoder().decode(Refusal.self, from: body))?.error == "removed"
    }
}
