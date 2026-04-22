import Foundation

struct ThreadService {
    let api: ApiClient

    func fetch(threadId: String, domains: [String] = []) async throws -> [ThreadMessage] {
        let path = "/api/conversations/\(threadId.urlEncoded)"
        var query: [URLQueryItem] = []
        if !domains.isEmpty {
            query.append(URLQueryItem(name: "domains", value: domains.joined(separator: ",")))
        }
        return try await api.get(path, query: query)
    }

    func delete(threadId: String) async throws {
        let _: EmptyResponse = try await api.delete("/api/conversations/\(threadId.urlEncoded)")
    }
}

struct MessageActionService {
    let api: ApiClient

    func markRead(threadId: String, domains: [String] = []) async throws {
        try await post("/api/conversations/\(threadId.urlEncoded)/read",
                       query: domains.queryItem)
    }

    func markUnread(threadId: String) async throws {
        try await post("/api/conversations/\(threadId.urlEncoded)/unread")
    }

    func star(threadId: String) async throws {
        try await post("/api/conversations/\(threadId.urlEncoded)/star")
    }

    func unstar(threadId: String) async throws {
        try await post("/api/conversations/\(threadId.urlEncoded)/unstar")
    }

    func archive(threadId: String) async throws {
        try await post("/api/conversations/\(threadId.urlEncoded)/archive")
    }

    func unarchive(threadId: String) async throws {
        try await post("/api/conversations/\(threadId.urlEncoded)/unarchive")
    }

    func pin(threadId: String) async throws {
        try await post("/api/conversations/\(threadId.urlEncoded)/pin")
    }

    func unpin(threadId: String) async throws {
        try await post("/api/conversations/\(threadId.urlEncoded)/unpin")
    }

    func snooze(threadId: String, until: Date) async throws {
        let iso = ISO8601DateFormatter().string(from: until)
        struct Body: Encodable { let until: String }
        let _: EmptyResponse = try await api.put(
            "/api/conversations/\(threadId.urlEncoded)/snooze",
            body: Body(until: iso)
        )
    }

    func unsnooze(threadId: String) async throws {
        let _: EmptyResponse = try await api.delete(
            "/api/conversations/\(threadId.urlEncoded)/snooze"
        )
    }

    func feedback(senderEmail: String, action: FeedbackAction) async throws {
        struct Body: Encodable { let sender_email: String; let action: String }
        try await api.postVoid(
            "/api/mail/feedback",
            body: Body(sender_email: senderEmail, action: action.rawValue)
        )
    }

    private func post(_ path: String, query: [URLQueryItem] = []) async throws {
        struct Empty: Encodable {}
        try await api.postVoid(path, query: query, body: Empty())
    }
}

enum FeedbackAction: String, CaseIterable, Sendable {
    case markVIP = "mark_vip"
    case markImportant = "mark_important"
    case markSpam = "mark_spam"
    case archive
    case block
    case unblock

    var displayName: String {
        switch self {
        case .markVIP: return "标为 VIP"
        case .markImportant: return "标为重要"
        case .markSpam: return "标为垃圾邮件"
        case .archive: return "归档发件人"
        case .block: return "屏蔽发件人"
        case .unblock: return "解除屏蔽"
        }
    }

    var systemImage: String {
        switch self {
        case .markVIP: return "star.circle"
        case .markImportant: return "exclamationmark.circle"
        case .markSpam: return "xmark.bin"
        case .archive: return "archivebox"
        case .block: return "hand.raised"
        case .unblock: return "hand.raised.slash"
        }
    }
}

// MARK: helpers

private extension String {
    /// Percent-encodes a thread id for use as a URL path segment.
    /// `URLQueryAllowed` is too permissive — we use `urlPathAllowed` minus `/`.
    var urlEncoded: String {
        var allowed = CharacterSet.urlPathAllowed
        allowed.remove(charactersIn: "/")
        return addingPercentEncoding(withAllowedCharacters: allowed) ?? self
    }
}

private extension Array where Element == String {
    var queryItem: [URLQueryItem] {
        isEmpty ? [] : [URLQueryItem(name: "domains", value: joined(separator: ","))]
    }
}
