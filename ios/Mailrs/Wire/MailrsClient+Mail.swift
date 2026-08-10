import Foundation

/// Reading: lists, threads, flags, attachments, search.
///
/// Split out of `MailrsClient.swift` at the 500-line limit this
/// repository holds every language to — and which did not look at
/// `ios/` until now. One type, several subjects.
extension MailrsClient {

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
        let url = url("/api/conversations/\(MailrsClient.segment(threadId))")
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
        var verbName = "mark-not-junk"
        if junk { verbName = "mark-junk" }
        try await verb("POST", "/api/conversations/\(MailrsClient.segment(threadId))/\(verbName)")
    }


    /// `POST /api/conversations/{id}/read` and `/unread`.
    func setRead(threadId: String, _ read: Bool) async throws {
        var verbName = "unread"
        if read { verbName = "read" }
        try await verb("POST", "/api/conversations/\(MailrsClient.segment(threadId))/\(verbName)")
    }


    /// `POST /api/conversations/{id}/star` and `/unstar`.
    func setStarred(threadId: String, _ starred: Bool) async throws {
        var verbName = "unstar"
        if starred { verbName = "star" }
        try await verb("POST", "/api/conversations/\(MailrsClient.segment(threadId))/\(verbName)")
    }


    /// Pin a thread to the top of the list, or let it back down.
    ///
    /// `pinned` is a declared column with an axis of its own and the
    /// web has drawn pinned rows first since it had rows; this client
    /// decoded the field and ignored it, so a thread pinned at the desk
    /// was buried on the phone. Same shape as star: POST, no body, 204.
    func setPinned(threadId: String, _ pinned: Bool) async throws {
        var verbName = "unpin"
        if pinned { verbName = "pin" }
        try await verb("POST", "/api/conversations/\(MailrsClient.segment(threadId))/\(verbName)")
    }


    /// Where a thread belongs: Inbox, Notifications or Promotions.
    ///
    /// The server has had `mark-notification`, `mark-promotion` and
    /// `move-to-inbox` all along and this client reached none of them —
    /// so a thread the classifier put in the wrong bucket stayed there,
    /// with no gesture on the phone that could move it.
    func moveTo(threadId: String, bucket: MailBucket) async throws {
        try await verb(
            "POST",
            "/api/conversations/\(MailrsClient.segment(threadId))/\(bucket.verb)")
    }


    /// `POST /api/conversations/{id}/archive` — 204, no body.
    func archive(threadId: String) async throws {
        try await verb("POST", "/api/conversations/\(MailrsClient.segment(threadId))/archive")
    }


    /// `POST /api/conversations/{id}/unarchive`.
    func unarchive(threadId: String) async throws {
        try await verb("POST", "/api/conversations/\(MailrsClient.segment(threadId))/unarchive")
    }


    /// `DELETE /api/conversations/{id}`.
    ///
    /// Irreversible. `thread_actions.rs` unlinks the maildir files after
    /// clearing the kevy rows — there is no trash and nothing to restore
    /// from — which is why every caller of this asks first.
    func delete(threadId: String) async throws {
        try await verb("DELETE", "/api/conversations/\(MailrsClient.segment(threadId))")
    }


    /// `POST /api/mail/unsubscribe` — ask the server to leave the list.
    ///
    /// Backend: `crates/webapi/src/handlers/unsubscribe.rs`, pinned by
    /// `wire-contract/requests/unsubscribe.json`. The body names the
    /// message; the server takes the URL out of that message's own
    /// header and posts to it, so nothing of this device reaches the
    /// sender.
    ///
    /// Returns whether the sender's endpoint accepted it. A refusal
    /// arrives as `ok: false` with the status, not as an error — the
    /// request was fine and the far end was not, and the reader needs
    /// to be told which.
    func unsubscribe(threadId: String, uid: UInt32) async throws -> Bool {
        struct Body: Encodable {
            let thread_id: String
            let uid: UInt32
        }
        struct Result: Decodable {
            let ok: Bool
        }
        let body = try JSONEncoder().encode(Body(thread_id: threadId, uid: uid))
        let (data, response) = try await send(
            "POST", "/api/mail/unsubscribe", body: body, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        return (try? JSONDecoder().decode(Result.self, from: data))?.ok ?? false
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
}
