import Foundation

extension Wire {
    /// `401` from `POST /api/auth/login` for an account whose password an
    /// administrator cleared, with whom to ask for a reset code.
    /// Backend: crates/webapi/src/handlers/auth/session.rs — fn password_cleared_response
    struct PasswordCleared: Decodable {
        struct Contact: Decodable {
            let name: String
            let address: String
        }

        let error: String
        let contact: Contact
    }

    struct PasswordResetRequest: Encodable {
        let address: String
        let code: String
        let newPassword: String

        enum CodingKeys: String, CodingKey {
            case address, code
            case newPassword = "new_password"
        }
    }
}

extension MailrsClient {
    /// `POST /api/auth/password-reset`: a new password, with the code an
    /// issuer handed out. Nothing is signed in; sign in afterwards.
    func resetPassword(address: String, code: String, newPassword: String) async throws {
        let body = Wire.PasswordResetRequest(address: address, code: code, newPassword: newPassword)
        let (data, response) = try await send(
            "POST", "/api/auth/password-reset", body: try JSONEncoder().encode(body), authorized: false
        )
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        if (200..<300).contains(http.statusCode) { return }
        let code = Self.errorCode(data)
        if code == "invalid_reset_code" { throw MailrsError.invalidResetCode }
        // a refused password says why in its own words
        if http.statusCode == 400, let reason = code { throw MailrsError.transport(reason) }
        throw MailrsError.server(status: http.statusCode)
    }
}
