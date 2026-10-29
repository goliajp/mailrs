import Foundation

extension MailrsClient {
    /// `POST /api/auth/confirm` — confirm this session's password, as the
    /// server asks before changes a stolen session could abuse. Lasts ten
    /// minutes on the server.
    ///
    /// Backend: crates/webapi/src/session/confirm.rs — `confirm`.
    func confirm(password: String, code: String?) async throws {
        var body: [String: String] = ["password": password]
        if let code { body["code"] = code }
        let (data, response) = try await send(
            "POST", "/api/auth/confirm",
            body: try JSONSerialization.data(withJSONObject: body), authorized: true
        )
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        if http.statusCode == 204 { return }
        let json = (try? JSONSerialization.jsonObject(with: data) as? [String: Any]) ?? [:]
        switch json["error"] as? String {
        case "confirm_failed":
            throw MailrsError.confirmFailed(remaining: json["remaining"] as? Int ?? 0)
        case "totp_required":
            throw MailrsError.totpRequired
        case "session_revoked":
            throw MailrsError.sessionEnded
        default:
            throw MailrsError.server(status: http.statusCode)
        }
    }
}
