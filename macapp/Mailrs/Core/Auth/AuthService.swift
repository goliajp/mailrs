import Foundation

struct AuthService {
    let api: ApiClient

    func login(address: String, password: String, totpCode: String?) async throws -> LoginOutcome {
        let req = LoginRequest(address: address, password: password, totp_code: totpCode)
        let res: LoginResponseDTO = try await api.post("/api/auth/login", body: req)

        if res.requires_totp == true {
            return .totpRequired
        }

        guard let token = res.token,
              let addr = res.address,
              let perms = res.permissions,
              let domains = res.accessible_domains else {
            throw ApiError.malformed("Login response missing token or user fields")
        }

        let info = AuthInfo(
            token: token,
            address: addr,
            display_name: res.display_name,
            permissions: perms,
            accessible_domains: domains,
            send_as: res.send_as
        )
        return .success(info)
    }

    /// Verifies the current bearer token is still valid. Returns refreshed user metadata.
    /// 401 errors propagate (the ApiClient will have already triggered signOut on the AuthStore).
    func me(currentToken: String) async throws -> AuthInfo {
        let res: MeResponse = try await api.get("/api/auth/me")
        return AuthInfo(
            token: currentToken,
            address: res.address,
            display_name: res.display_name,
            permissions: res.permissions,
            accessible_domains: res.accessible_domains,
            send_as: res.send_as
        )
    }

    func logout() async throws {
        try await api.postVoid("/api/auth/logout", body: EmptyBody())
    }
}

struct EmptyBody: Encodable, Sendable {}

struct MeResponse: Decodable, Sendable {
    let address: String
    let display_name: String?
    let permissions: [String]
    let accessible_domains: [String]
    let send_as: [String]?
}
