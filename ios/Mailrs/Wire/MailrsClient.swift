import Foundation

enum MailrsError: Error, LocalizedError, Equatable {
    case badCredentials
    case needsTotp
    case server(status: Int)
    case decoding(String)
    case transport(String)
    /// A write the server could not take while restarting. Never repeated
    /// here — sending, moving or deleting twice could do it twice.
    case restarting
    /// The server wants the password typed again first (`confirm_required`).
    case confirmRequired
    /// A wrong password or code at the confirmation, with the tries left
    /// before the server ends the session.
    case confirmFailed(remaining: Int)
    /// The account has an authenticator; the confirmation needs its code.
    case totpRequired
    /// Too many wrong confirmations: the server ended this session.
    case sessionEnded

    var errorDescription: String? {
        switch self {
        case .badCredentials: "That address and password did not match."
        case .needsTotp: "Enter the six-digit code from your authenticator."
        case let .server(status): "The server answered \(status)."
        case let .decoding(what): "The server sent something unexpected: \(what)"
        case let .transport(what): what
        case .restarting: "The server is restarting. Try again in a moment."
        case .confirmRequired: "Enter your password to continue."
        case let .confirmFailed(remaining) where remaining == 1:
            "Wrong password or code. 1 try left before you are signed out."
        case let .confirmFailed(remaining):
            "Wrong password or code. \(remaining) tries left before you are signed out."
        case .totpRequired: "Enter the code from your authenticator app."
        case .sessionEnded: "Signed out after too many wrong passwords."
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

    /// Told on a 403 that is not a request for the password: the account
    /// may have lost a right.
    var onForbidden: (@Sendable () -> Void)?

    func setOnForbidden(_ handler: @escaping @Sendable () -> Void) {
        onForbidden = handler
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
            // With the key path *and* what actually arrived. A GitHub
            // notification would not open on the phone and the message
            // said only "the server sent something unexpected", which
            // names neither the field nor the value — so the next
            // occurrence is a report you can act on rather than a
            // reproduction you have to hunt for.
            let head = String(decoding: data.prefix(240), as: UTF8.self)
            throw MailrsError.decoding("\(path) — \(error) — got: \(head)")
        }
    }


    /// One path segment, safe to interpolate.
    ///
    /// A thread id is a Message-ID and can hold anything a sender put
    /// there — GitHub's are
    /// `goliajp/kevy/check-suites/CS_kw…/1786297239@github.com`, four
    /// slashes deep. Interpolated raw they became four path segments,
    /// the request reached some other route, and what came back decoded
    /// into nothing: "the server sent something unexpected" on every
    /// GitHub notification. The web client had always encoded them.
    /// `baseURL` + an already-encoded path.
    ///
    /// Not `appendingPathComponent`, which treats its argument as a
    /// literal component and escapes the `%` — turning the `%2F` of an
    /// encoded thread id into `%252F` and breaking it a second way.
    nonisolated func url(_ encodedPath: String) -> URL {
        URL(string: baseURL.absoluteString + encodedPath) ?? baseURL
    }

    static func segment(_ raw: String) -> String {
        raw.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(
            CharacterSet(charactersIn: "-._~")
        )) ?? raw
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
        try await send(method, url: url(path), body: body, authorized: authorized)
    }


    /// A 503 is the server restarting during a deploy. A read waits the
    /// `Retry-After` it gives and goes once more; a write is never
    /// repeated, and says the server is restarting instead.
    func send(
        _ method: String, url: URL, body: Data?, authorized: Bool,
        contentType: String = "application/json", retried: Bool = false
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
        let answer: (Data, URLResponse)
        do {
            answer = try await session.data(for: request)
        } catch {
            throw MailrsError.transport(error.localizedDescription)
        }
        guard let http = answer.1 as? HTTPURLResponse else { return answer }
        if http.statusCode == 403, Self.errorCode(answer.0) == "confirm_required" {
            throw MailrsError.confirmRequired
        }
        if http.statusCode == 403 { onForbidden?() }
        guard http.statusCode == 503 else { return answer }
        guard method == "GET", !retried else { throw MailrsError.restarting }
        let wait = Self.retryAfterMs(http.value(forHTTPHeaderField: "Retry-After"))
        try? await Task.sleep(for: .milliseconds(wait))
        return try await send(
            method, url: url, body: body, authorized: authorized,
            contentType: contentType, retried: true
        )
    }

    /// The `error` field of a JSON error body, if there is one.
    nonisolated static func errorCode(_ body: Data) -> String? {
        (try? JSONSerialization.jsonObject(with: body) as? [String: Any])?["error"] as? String
    }

    /// How long a read waits before asking a restarting server again: its
    /// `Retry-After` in seconds, at most thirty; five when it gives none.
    nonisolated static func retryAfterMs(_ header: String?) -> Int {
        guard let seconds = header.flatMap({ Int($0.trimmingCharacters(in: .whitespaces)) }),
              seconds > 0 else { return 5_000 }
        return min(seconds * 1000, 30_000)
    }
}

