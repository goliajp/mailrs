import Foundation

/// Running it: the queue, DMARC, the audit log, API keys.
///
/// Split out of `MailrsClient.swift` at the 500-line limit this
/// repository holds every language to — and which did not look at
/// `ios/` until now. One type, several subjects.
extension MailrsClient {

    /// `GET /api/admin/queues`.
    func queue() async throws -> [Wire.QueueJob] {
        let list: Wire.QueueList = try await getJSON("/api/admin/queues")
        return list.items
    }


    /// `GET /api/admin/suppressions`.
    func suppressions() async throws -> [String] {
        let list: Wire.SuppressionList = try await getJSON("/api/admin/suppressions")
        return list.items
    }


    /// `DELETE /api/admin/suppressions` — clears the whole set. The
    /// endpoint takes no address: it is all or nothing, which is why
    /// the screen asks before calling it.
    func clearSuppressions() async throws {
        try await verb("DELETE", "/api/admin/suppressions")
    }


    /// `GET /api/admin/dmarc/reports`.
    func dmarcReports() async throws -> [Wire.DmarcReport] {
        let list: Wire.DmarcReportList = try await getJSON("/api/admin/dmarc/reports")
        return list.items
    }


    /// `GET /api/admin/dmarc/sources` — the rollup, with the window's
    /// own totals so the screen does not have to add up the rows and
    /// hope it matched what the server counted.
    func dmarcSources() async throws -> Wire.DmarcSourceList {
        try await getJSON("/api/admin/dmarc/sources")
    }


    /// `GET /api/admin/audit-log?limit=&action=`.
    ///
    /// `action` is a prefix on the server, so `alias` matches
    /// `alias.create` and `alias.delete` — the filter is a family, not
    /// an exact verb, and the screen offers it that way.
    func auditLog(limit: Int = 100, actionPrefix: String? = nil) async throws -> [Wire.AuditRow] {
        var components = URLComponents(
            url: baseURL.appendingPathComponent("/api/admin/audit-log"),
            resolvingAgainstBaseURL: false
        )
        var query = [URLQueryItem(name: "limit", value: String(limit))]
        if let actionPrefix, !actionPrefix.isEmpty {
            query.append(URLQueryItem(name: "action", value: actionPrefix))
        }
        components?.queryItems = query
        guard let url = components?.url else { throw MailrsError.transport("Bad audit URL.") }
        let (data, response) = try await send("GET", url: url, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
        do {
            return try JSONDecoder().decode(Wire.AuditList.self, from: data).items
        } catch {
            throw MailrsError.decoding("audit log — \(error)")
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
    // MARK: Agent keys

    func agentKeys() async throws -> [Wire.AgentKey] {
        struct Envelope: Decodable { let items: [Wire.AgentKey] }
        let envelope: Envelope = try await getJSON("/api/agent/keys")
        return envelope.items
    }


    /// The response is the only place the secret ever exists outside
    /// this call — the server keeps eight characters of it.
    func createAgentKey(name: String, scopes: [String]) async throws -> Wire.CreateAgentKeyResponse {
        let request = Wire.CreateAgentKeyRequest(name: name, scopes: scopes)
        let (data, response) = try await send(
            "POST", "/api/agent/keys",
            body: try JSONEncoder().encode(request), authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
        do {
            return try JSONDecoder().decode(Wire.CreateAgentKeyResponse.self, from: data)
        } catch {
            throw MailrsError.decoding("agent key — \(error)")
        }
    }


    func deleteAgentKey(id: Int64) async throws {
        try await verb("DELETE", "/api/agent/keys/\(id)")
    }
}
