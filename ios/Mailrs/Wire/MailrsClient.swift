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
    /// A message that starts its own thread — no `in_reply_to`, no
    /// `reply_to_thread_id`.
    @discardableResult
    func sendNew(
        to recipients: [String], cc: [String] = [], subject: String, body: String
    ) async throws -> Wire.SendResponse {
        try await post(Wire.SendRequest(
            to: recipients, cc: cc, subject: subject, body: body,
            inReplyTo: nil, replyToThreadId: nil,
            forwardMessageId: nil, forwardAttachmentsFrom: nil
        ))
    }

    @discardableResult
    func sendReply(
        to recipients: [String],
        cc: [String] = [],
        subject: String,
        body: String,
        inReplyTo: String?,
        threadId: String
    ) async throws -> Wire.SendResponse {
        return try await post(Wire.SendRequest(
            to: recipients, cc: cc, subject: subject, body: body,
            inReplyTo: inReplyTo, replyToThreadId: threadId,
            forwardMessageId: nil, forwardAttachmentsFrom: nil
        ))
    }

    /// `POST /api/mail/send-multipart` — the send that carries files.
    /// Backend: `crates/webapi/src/handlers/send.rs` —
    /// `send_message_multipart`; `to` and `attachments` are repeated
    /// fields, the rest match the JSON names.
    @discardableResult
    func sendMultipart(
        to recipients: [String],
        subject: String,
        body: String,
        attachments: [MultipartForm.FilePart],
        inReplyTo: String? = nil,
        replyToThreadId: String? = nil,
        forwardMessageId: String? = nil,
        forwardAttachmentsFrom: UInt32? = nil
    ) async throws -> Wire.SendResponse {
        let boundary = "mailrs-\(UUID().uuidString)"
        var fields: [(String, String)] = recipients.map { ("to", $0) }
        fields.append(("subject", subject))
        fields.append(("body", body))
        // Same optionality contract as the JSON route: absent, never
        // empty — the handler filters empties for some fields and not
        // others, and absent is the shape it always understands.
        if let inReplyTo { fields.append(("in_reply_to", inReplyTo)) }
        if let replyToThreadId { fields.append(("reply_to_thread_id", replyToThreadId)) }
        if let forwardMessageId { fields.append(("forward_message_id", forwardMessageId)) }
        if let forwardAttachmentsFrom {
            fields.append(("forward_attachments_from", String(forwardAttachmentsFrom)))
        }
        let form = MultipartForm.encode(fields: fields, files: attachments, boundary: boundary)
        let url = baseURL.appendingPathComponent("/api/mail/send-multipart")
        let (data, response) = try await send(
            "POST", url: url, body: form, authorized: true,
            contentType: "multipart/form-data; boundary=\(boundary)"
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
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

    /// A forward: no threading fields — a forward starts its own thread
    /// — and the original travels by reference, with the server
    /// appending body and attachments from the raw .eml.
    @discardableResult
    func sendForward(
        to recipients: [String],
        subject: String,
        body: String,
        forwardMessageId: String,
        forwardAttachmentsFrom: UInt32?
    ) async throws -> Wire.SendResponse {
        return try await post(Wire.SendRequest(
            to: recipients, cc: [], subject: subject, body: body,
            inReplyTo: nil, replyToThreadId: nil,
            forwardMessageId: forwardMessageId,
            forwardAttachmentsFrom: forwardAttachmentsFrom
        ))
    }

    /// The one place a compose form becomes a request, so new messages
    /// and replies cannot drift apart in how they read the answer.
    private func post(_ payload: Wire.SendRequest) async throws -> Wire.SendResponse {
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

    /// `GET /api/admin/aliases` — Backend:
    /// `crates/webapi/src/handlers/admin_directory.rs::list_aliases`.
    func aliases() async throws -> [Wire.Alias] {
        let list: Wire.AliasList = try await getJSON("/api/admin/aliases")
        return list.items
    }

    /// `POST /api/admin/aliases`.
    func addAlias(_ request: Wire.AddAliasRequest) async throws {
        let (_, response) = try await send(
            "POST", "/api/admin/aliases",
            body: try JSONEncoder().encode(request), authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }

    /// `DELETE /api/admin/aliases/{id}`.
    func deleteAlias(id: Int64) async throws {
        try await verb("DELETE", "/api/admin/aliases/\(id)")
    }

    /// `GET /api/icon/{domain}` — the sender-avatar cascade.
    /// Backend: `crates/webapi/src/handlers/icon.rs`. Bytes on a hit,
    /// **204 on a miss** rather than 404, so "this domain has no
    /// icon" is an answer rather than an error.
    func icon(domain: String) async throws -> Data? {
        let encoded = domain.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? domain
        let url = baseURL.appendingPathComponent("/api/icon/\(encoded)")
        let (data, response) = try await send("GET", url: url, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else { return nil }
        guard http.statusCode == 200, !data.isEmpty else { return nil }
        return data
    }

    /// `GET /api/contacts?q=&limit=` — a bare array of `Name <email>`
    /// strings. Backend: `crates/webapi/src/handlers/prefs_misc.rs` —
    /// `get_contacts`, backed by the per-user contacts hash the ingest
    /// path maintains.
    func contacts(matching query: String, limit: Int = 5) async throws -> [String] {
        var components = URLComponents(
            url: baseURL.appendingPathComponent("/api/contacts"),
            resolvingAgainstBaseURL: false
        )
        components?.queryItems = [
            URLQueryItem(name: "q", value: query),
            URLQueryItem(name: "limit", value: String(limit)),
        ]
        guard let url = components?.url else { throw MailrsError.transport("Bad contacts URL.") }
        let (data, response) = try await send("GET", url: url, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
        do {
            return try JSONDecoder().decode([String].self, from: data)
        } catch {
            throw MailrsError.decoding("contacts — \(error)")
        }
    }

    /// `GET /api/conversations/unseen-count` — `{"count": N}`.
    ///
    /// Backend: `conversations.rs::get_unseen_count`, which counts
    /// unread across the whole non-junk mailbox — not the page in hand
    /// and not the active list. That is the number an app icon should
    /// wear: the icon is visible from the home screen, where "which
    /// list was open" is not a meaningful scope.
    func unseenCount() async throws -> Int {
        struct Body: Decodable { let count: Int }
        let body: Body = try await getJSON("/api/conversations/unseen-count")
        return body.count
    }

    /// `GET /api/mail/sent` — the sent axis, a bare array.
    func sentMessages() async throws -> [Wire.SentMessage] {
        try await getJSON("/api/mail/sent")
    }

    /// `GET /api/mail/sends` — the delivery-status projection.
    func sends() async throws -> [Wire.Send] {
        try await getJSON("/api/mail/sends")
    }

    private func getJSON<T: Decodable>(_ path: String) async throws -> T {
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

    /// `POST /api/push/tokens` — hand the server this device's address.
    ///
    /// Backend: `crates/webapi/src/handlers/push.rs` —
    /// `RegisterPushTokenRequest`, pinned by
    /// `wire-contract/requests/push-register.json`.
    func registerPushToken(_ token: String) async throws {
        let body = try JSONEncoder().encode(["token": token, "platform": "ios"])
        let (_, response) = try await send("POST", "/api/push/tokens", body: body, authorized: true)
        guard let http = response as? HTTPURLResponse,
              (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }

    /// `GET /api/mail/drafts` — newest first, sorted server-side by
    /// `updated_at`.
    func drafts() async throws -> [Wire.Draft] {
        let (data, response) = try await send("GET", "/api/mail/drafts", body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode([Wire.Draft].self, from: data)
        } catch {
            throw MailrsError.decoding("drafts — \(error)")
        }
    }

    /// `POST /api/mail/drafts` — returns the id, new or the one given.
    func saveDraft(_ draft: Wire.SaveDraftRequest) async throws -> Int64 {
        let (data, response) = try await send(
            "POST", "/api/mail/drafts", body: try JSONEncoder().encode(draft), authorized: true
        )
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode(Wire.SaveDraftResponse.self, from: data).id
        } catch {
            throw MailrsError.decoding("save draft — \(error)")
        }
    }

    /// `DELETE /api/mail/drafts/{id}`.
    func deleteDraft(id: Int64) async throws {
        try await verb("DELETE", "/api/mail/drafts/\(id)")
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

    /// `POST /api/conversations/{id}/mark-junk` and `/mark-not-junk`.
    ///
    /// More than a move: the server trains the Bayes classifier on the
    /// verdict and (for not-junk) whitelists the sender, which is why
    /// this is worth reaching for over archive when something is spam.
    func setJunk(threadId: String, _ junk: Bool) async throws {
        try await verb("POST", "/api/conversations/\(threadId)/\(junk ? "mark-junk" : "mark-not-junk")")
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
