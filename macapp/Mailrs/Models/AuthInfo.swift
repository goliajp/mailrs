import Foundation

struct AuthInfo: Codable, Equatable, Sendable {
    let token: String
    let address: String
    let display_name: String?
    let permissions: [String]
    let accessible_domains: [String]
    let send_as: [String]?

    var displayName: String { display_name ?? address }
    var accessibleDomains: [String] { accessible_domains }
    var sendAs: [String] { send_as ?? [address] }
}

struct LoginRequest: Encodable, Sendable {
    let address: String
    let password: String
    let totp_code: String?
}

struct LoginResponseDTO: Decodable, Sendable {
    let requires_totp: Bool?
    let token: String?
    let address: String?
    let display_name: String?
    let permissions: [String]?
    let accessible_domains: [String]?
    let send_as: [String]?
}

enum LoginOutcome: Sendable {
    case success(AuthInfo)
    case totpRequired
}
