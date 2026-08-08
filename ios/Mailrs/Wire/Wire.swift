import Foundation

/// The shapes the mailrs REST API actually sends.
///
/// Written against the Rust handlers, not against `openapi.json` — that
/// file has drifted before, and the web client shipped nine schemas
/// disagreeing with the backend because they were written from it
/// (`.claude/rules/frontend/wire-schema-verification.md`). Each type
/// below names the handler it mirrors so the next person can check.
enum Wire {
    /// Backend: `crates/webapi/src/handlers/auth/session.rs` — `LoginRequest`.
    struct LoginRequest: Encodable {
        let address: String
        let password: String
        let totpCode: String?

        enum CodingKeys: String, CodingKey {
            case address
            case password
            case totpCode = "totp_code"
        }
    }


    /// Backend: `crates/webapi/src/handlers/auth/session.rs` — `LoginResponse`.
    ///
    /// The handler also has a `{ requires_totp: true }` short-circuit
    /// before a session is issued, which is why the decode is attempted
    /// as `TotpChallenge` first in `MailrsClient.logIn`.
    struct LoginResponse: Decodable {
        let address: String
        let displayName: String
        let permissions: [String]
        let token: String

        enum CodingKeys: String, CodingKey {
            case address
            case displayName = "display_name"
            case permissions
            case token
        }
    }


    struct TotpChallenge: Decodable {
        let requiresTotp: Bool

        enum CodingKeys: String, CodingKey {
            case requiresTotp = "requires_totp"
        }
    }
}
