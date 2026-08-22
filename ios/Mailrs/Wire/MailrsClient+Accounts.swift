import Foundation

extension MailrsClient {
    /// `GET /api/accounts/external` — the mailboxes this person has
    /// connected.
    func externalAccounts() async throws -> [Wire.ExternalAccount] {
        try await getJSON("/api/accounts/external")
    }

    /// `GET /api/accounts/external/settings` — what to fill in for an
    /// address, before anything is saved.
    ///
    /// Asked as the address is typed so the secret field can be
    /// labelled with the provider's own word for what it wants. Typing
    /// a login password into a field labelled 授权码 is a mistake
    /// somebody recovers from; typing it into one labelled "Password"
    /// and being told `LOGIN failed` is not.
    func accountSettings(for email: String) async throws -> Wire.AccountSettings {
        let q = email.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? email
        return try await getJSON("/api/accounts/external/settings?email=\(q)")
    }

    /// `POST /api/accounts/external` — connect one.
    ///
    /// An address and a secret. Everything else the server fills in
    /// from its provider table, or discovers from DNS.
    func connectAccount(email: String, secret: String, name: String) async throws {
        var body: [String: String] = ["email": email, "secret": secret]
        if !name.isEmpty { body["display_name"] = name }
        _ = try await send(
            "POST", "/api/accounts/external",
            body: try JSONSerialization.data(withJSONObject: body), authorized: true)
    }

    /// `DELETE /api/accounts/external/{id}` — disconnect one.
    func disconnectAccount(id: String) async throws {
        let safe = id.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? id
        _ = try await send("DELETE", "/api/accounts/external/\(safe)", body: nil, authorized: true)
    }
}
