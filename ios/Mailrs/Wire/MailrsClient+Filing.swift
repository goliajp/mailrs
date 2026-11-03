import Foundation

extension Wire {
    /// One of the reader's labels, from `GET /api/labels`.
    struct LabelItem: Codable, Equatable, Hashable, Identifiable, Sendable {
        let id: String
        let name: String
        let color: String?
    }

    /// `GET /api/quarantine`: the held conversations, in an envelope.
    struct HeldList: Decodable, Sendable {
        let items: [Conversation]
    }
}

/// Filing beyond the inbox: the Trash, Review and labels.
///
/// Putting in the Trash, taking out of it and labelling go through
/// `batch` (actions `delete`, `untrash`, `label`, `unlabel`), as the web
/// and Android send them. These are the rest, each a route of its own.
extension MailrsClient {

    /// `GET /api/labels` — a bare array.
    func labels() async throws -> [Wire.LabelItem] {
        let (data, response) = try await send("GET", "/api/labels", body: nil, authorized: true)
        try Self.ok(response)
        do {
            return try JSONDecoder().decode([Wire.LabelItem].self, from: data)
        } catch {
            throw MailrsError.decoding("labels — \(error)")
        }
    }

    /// `GET /api/quarantine` — newest first, older ones by `before`.
    func held(limit: Int = 50, before: Int64? = nil) async throws -> [Wire.Conversation] {
        var components = URLComponents(
            url: baseURL.appendingPathComponent("/api/quarantine"), resolvingAgainstBaseURL: false)
        var items = [URLQueryItem(name: "limit", value: String(limit))]
        if let before { items.append(URLQueryItem(name: "before_ts", value: String(before))) }
        components?.queryItems = items
        guard let url = components?.url else { throw MailrsError.transport("Bad URL.") }
        let (data, response) = try await send("GET", url: url, body: nil, authorized: true)
        try Self.ok(response)
        do {
            return try JSONDecoder().decode(Wire.HeldList.self, from: data).items
        } catch {
            throw MailrsError.decoding("held list — \(error)")
        }
    }

    /// `POST /api/quarantine/{id}/release` — it was not fraud.
    func release(threadId: String) async throws {
        try await verb("POST", "/api/quarantine/\(MailrsClient.segment(threadId))/release")
    }

    /// `DELETE /api/conversations/{id}/forever` — files and all, for
    /// good. Needs a recent password confirmation; callers go through
    /// `Session.confirmed`.
    func deleteForever(threadId: String) async throws {
        try await verb("DELETE", "/api/conversations/\(MailrsClient.segment(threadId))/forever")
    }

    nonisolated static func ok(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
    }
}
