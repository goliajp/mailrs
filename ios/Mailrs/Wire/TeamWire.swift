import Foundation

/// A team conversation, read only: its notes, and who is on it.
/// Backend: `crates/webapi/src/handlers/shared_inbox/` — `Overview`, `notes::Note`.
enum TeamWire {
    /// `gid` and `conv` from a row's `shared_conv` ("1:7"); nil when it is
    /// not a team copy or the value is not two numbers.
    struct Conv: Equatable, Sendable {
        let gid: Int
        let conv: Int

        init?(_ sharedConv: String?) {
            let parts = (sharedConv ?? "").split(separator: ":")
            guard parts.count == 2, let gid = Int(parts[0]), let conv = Int(parts[1]) else { return nil }
            self.gid = gid
            self.conv = conv
        }

        var path: String { "/api/shared/\(gid)/\(conv)" }
    }

    struct Note: Decodable, Identifiable, Equatable, Sendable {
        let id: Int
        let at: Int64
        let by: String
        let editedAt: Int64?
        let text: String

        enum CodingKeys: String, CodingKey {
            case id, at, by, text
            case editedAt = "edited_at"
        }
    }

    struct DraftPointer: Decodable, Equatable, Sendable {
        let author: String
    }

    struct Overview: Decodable, Equatable, Sendable {
        let assignee: String?
        let draft: DraftPointer?
    }

    /// What a reply sheet says about who is on the conversation: someone
    /// else writing a reply first, then someone else assigned; nil when
    /// it is the reader's own or nobody's.
    enum ReplyHint: Equatable {
        case writing(String)
        case assigned(String)
    }

    static func replyHint(_ overview: Overview, me: String) -> ReplyHint? {
        let mine = me.lowercased()
        if let author = overview.draft?.author, !author.isEmpty, author.lowercased() != mine {
            return .writing(localPart(author))
        }
        if let assignee = overview.assignee, !assignee.isEmpty, assignee.lowercased() != mine {
            return .assigned(localPart(assignee))
        }
        return nil
    }

    /// `bob` for `bob@golia.jp`, as the row names an assignee.
    static func localPart(_ address: String) -> String {
        guard let at = address.firstIndex(of: "@"), at != address.startIndex else { return address }
        return String(address[..<at])
    }
}

@MainActor
extension MailrsClient {
    func teamNotes(_ c: TeamWire.Conv) async throws -> [TeamWire.Note] {
        try await getJSON(c.path + "/notes")
    }

    func teamOverview(_ c: TeamWire.Conv) async throws -> TeamWire.Overview {
        try await getJSON(c.path)
    }
}
