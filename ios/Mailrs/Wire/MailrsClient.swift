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
    private let baseURL: URL
    private let session: URLSession
    private var token: String?

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

    /// `GET /api/conversations` — a bare array, see `Wire.Conversation`.
    func conversations(
        axes: MailListAxes, limit: Int = 50, before: Int64? = nil
    ) async throws -> [Wire.Conversation] {
        var components = URLComponents(url: baseURL.appendingPathComponent("/api/conversations"),
                                       resolvingAgainstBaseURL: false)
        var items = axes.queryItems + [URLQueryItem(name: "limit", value: String(limit))]
        if let before {
            items.append(URLQueryItem(name: "before_ts", value: String(before)))
        }
        components?.queryItems = items
        guard let url = components?.url else { throw MailrsError.transport("Bad URL.") }
        let (data, response) = try await send("GET", url: url, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode([Wire.Conversation].self, from: data)
        } catch {
            throw MailrsError.decoding("conversation list — \(error)")
        }
    }

    /// `GET /api/conversations/{thread_id}` — a bare array, like the list.
    func messages(threadId: String) async throws -> [Wire.Message] {
        let url = baseURL.appendingPathComponent("/api/conversations/\(threadId)")
        let (data, response) = try await send("GET", url: url, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode([Wire.Message].self, from: data)
        } catch {
            throw MailrsError.decoding("thread messages — \(error)")
        }
    }

    /// `POST /api/mail/send`.
    ///
    /// The handler answers 200 with `success: false` for a send it
    /// accepted but could not queue, so the status code alone is not the
    /// answer — a reply that never left would otherwise look sent.
    @discardableResult
    func sendReply(
        to recipients: [String],
        cc: [String] = [],
        subject: String,
        body: String,
        inReplyTo: String?,
        threadId: String
    ) async throws -> Wire.SendResponse {
        let payload = Wire.SendRequest(
            to: recipients, cc: cc, subject: subject, body: body,
            inReplyTo: inReplyTo, replyToThreadId: threadId
        )
        let (data, response) = try await send(
            "POST", "/api/mail/send", body: try JSONEncoder().encode(payload), authorized: true
        )
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        let result: Wire.SendResponse
        do {
            result = try JSONDecoder().decode(Wire.SendResponse.self, from: data)
        } catch {
            throw MailrsError.decoding("send response — \(error)")
        }
        guard result.success else {
            throw MailrsError.transport(result.message ?? "The server did not queue the message.")
        }
        return result
    }

    /// `GET /api/conversations/search` — ranked, and already hydrated
    /// into the same row shape the list uses.
    ///
    /// The order is the ranking. `search_conversations` hydrates by
    /// walking the hit ids (`thread_ids.iter().filter_map`), so the array
    /// arrives in relevance order and re-sorting it by date would throw
    /// the ranking away.
    func search(query: String, axes: MailListAxes, limit: Int = 50) async throws -> [Wire.Conversation] {
        var components = URLComponents(url: baseURL.appendingPathComponent("/api/conversations/search"),
                                       resolvingAgainstBaseURL: false)
        // The same axes the list uses. `SearchQuery` takes the identical
        // four, and scoping the search differently from the list it was
        // typed into is how searching Junk returns Inbox.
        components?.queryItems = axes.queryItems + [
            URLQueryItem(name: "q", value: query),
            URLQueryItem(name: "limit", value: String(limit)),
        ]
        guard let url = components?.url else { throw MailrsError.transport("Bad URL.") }
        let (data, response) = try await send("GET", url: url, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode([Wire.Conversation].self, from: data)
        } catch {
            throw MailrsError.decoding("search results — \(error)")
        }
    }

    /// `GET /api/mail/messages/{uid}/attachments/{index}` — the bytes.
    ///
    /// Not the `/content` sibling: that one answers JSON with extracted
    /// text and only for `text/*`, `application/json` and
    /// `application/xml`, returning `success: false` for everything else.
    /// This one is the file.
    ///
    /// `index` is the position in the message's attachment array — the
    /// handler resolves it as `attachments.get(index)` and there is no id
    /// on the wire to use instead.
    func attachment(uid: UInt32, index: Int) async throws -> Data {
        let path = "/api/mail/messages/\(uid)/attachments/\(index)"
        let (data, response) = try await send("GET", path, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        return data
    }

    /// `POST /api/conversations/{id}/read` and `/unread`.
    func setRead(threadId: String, _ read: Bool) async throws {
        try await verb("POST", "/api/conversations/\(threadId)/\(read ? "read" : "unread")")
    }

    /// `POST /api/conversations/{id}/star` and `/unstar`.
    func setStarred(threadId: String, _ starred: Bool) async throws {
        try await verb("POST", "/api/conversations/\(threadId)/\(starred ? "star" : "unstar")")
    }

    /// `POST /api/conversations/{id}/archive` — 204, no body.
    func archive(threadId: String) async throws {
        try await verb("POST", "/api/conversations/\(threadId)/archive")
    }

    /// `POST /api/conversations/{id}/unarchive`.
    func unarchive(threadId: String) async throws {
        try await verb("POST", "/api/conversations/\(threadId)/unarchive")
    }

    /// `DELETE /api/conversations/{id}`.
    ///
    /// Irreversible. `thread_actions.rs` unlinks the maildir files after
    /// clearing the kevy rows — there is no trash and nothing to restore
    /// from — which is why every caller of this asks first.
    func delete(threadId: String) async throws {
        try await verb("DELETE", "/api/conversations/\(threadId)")
    }

    private func verb(_ method: String, _ path: String) async throws {
        let (_, response) = try await send(method, path, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
    }

    private func send(
        _ method: String, _ path: String, body: Data?, authorized: Bool
    ) async throws -> (Data, URLResponse) {
        try await send(method, url: baseURL.appendingPathComponent(path), body: body, authorized: authorized)
    }

    private func send(
        _ method: String, url: URL, body: Data?, authorized: Bool
    ) async throws -> (Data, URLResponse) {
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.httpBody = body
        if body != nil {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
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
