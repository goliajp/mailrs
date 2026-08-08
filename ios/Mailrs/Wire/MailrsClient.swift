import Foundation

enum MailrsError: Error, LocalizedError, Equatable {
    case badCredentials
    case needsTotp
    case server(status: Int)
    case decoding(String)
    case transport(String)

    var errorDescription: String? {
        switch self {
        case .badCredentials: "That address and password did not match."
        case .needsTotp: "Enter the six-digit code from your authenticator."
        case let .server(status): "The server answered \(status)."
        case let .decoding(what): "The server sent something unexpected: \(what)"
        case let .transport(what): what
        }
    }
}

/// Talks to a mailrs instance.
///
/// One session token, sent as `Authorization: Bearer`. The web client
/// keeps the same token in `localStorage` under `mailrs_auth`; this holds
/// it in the Keychain instead, because an app's container is readable
/// from a backup and `UserDefaults` is not a place for a credential.
actor MailrsClient {
    // Internal rather than private: `private` in Swift is file-scoped,
    // and the calls that use these now live in MailrsClient+*.swift.
    let baseURL: URL

    let session: URLSession

    var token: String?


    init(baseURL: URL, session: URLSession = .shared, token: String? = nil) {
        self.baseURL = baseURL
        self.session = session
        self.token = token
    }

    func setToken(_ token: String?) {
        self.token = token
    }


    /// `POST /api/auth/login`.
    ///
    /// The handler answers 401 for both "no such account" and "wrong
    /// password" — deliberately, so the response cannot be used to
    /// enumerate addresses — so this cannot say which it was either.
    func logIn(address: String, password: String, totpCode: String? = nil) async throws -> Wire.LoginResponse {
        let body = Wire.LoginRequest(address: address, password: password, totpCode: totpCode)
        let (data, response) = try await send(
            "POST", "/api/auth/login", body: try JSONEncoder().encode(body), authorized: false
        )
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        if http.statusCode == 401 { throw MailrsError.badCredentials }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        // The TOTP short-circuit comes back 200 with a different body, so
        // it has to be ruled out before the success decode — otherwise
        // "enter your code" surfaces as a decoding failure.
        if let challenge = try? JSONDecoder().decode(Wire.TotpChallenge.self, from: data),
           challenge.requiresTotp {
            throw MailrsError.needsTotp
        }
        do {
            let login = try JSONDecoder().decode(Wire.LoginResponse.self, from: data)
            token = login.token
            return login
        } catch {
            throw MailrsError.decoding("login response — \(error)")
        }
    }


    func getJSON<T: Decodable>(_ path: String) async throws -> T {
        let (data, response) = try await send("GET", path, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode(T.self, from: data)
        } catch {
            throw MailrsError.decoding("\(path) — \(error)")
        }
    }


    func verb(_ method: String, _ path: String) async throws {
        let (_, response) = try await send(method, path, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
    }


    func send(
        _ method: String, _ path: String, body: Data?, authorized: Bool
    ) async throws -> (Data, URLResponse) {
        try await send(method, url: baseURL.appendingPathComponent(path), body: body, authorized: authorized)
    }


    func send(
        _ method: String, url: URL, body: Data?, authorized: Bool,
        contentType: String = "application/json"
    ) async throws -> (Data, URLResponse) {
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.httpBody = body
        if body != nil {
            request.setValue(contentType, forHTTPHeaderField: "Content-Type")
        }
        if authorized, let token {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        do {
            return try await session.data(for: request)
        } catch {
            throw MailrsError.transport(error.localizedDescription)
        }
    }
}

