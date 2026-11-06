import Foundation

/// What a swipe on a conversation row does. The reader's setting, kept on
/// the server (`/api/settings/swipe`) so the web and both apps agree; the
/// choices and the default are the `swipe` block of `actions.json`.
enum SwipeChoice: String, Codable, CaseIterable, Sendable {
    case archive, trash, read, none

    /// The catalog key of its name in Settings.
    var titleKey: String {
        switch self {
        case .archive: "Archive"
        case .trash: "Move to Trash"
        case .read: "Mark read or unread"
        case .none: "Do nothing"
        }
    }

    /// The action it runs on `c`, or nil for none.
    ///
    /// In the Trash archive and trash both restore: a swipe never deletes,
    /// because deleting for good is the one thing nothing takes back.
    func action(for c: Wire.Conversation, in list: MailList) -> RowAction? {
        switch self {
        case .none:
            return nil
        case .read:
            if c.unreadCount > 0 { return .read }
            return .unread
        case .trash, .archive:
            if c.isTrashed || list == .trash { return .untrash }
            if self == .trash { return .trash }
            if c.archived { return .unarchive }
            return .archive
        }
    }
}

/// Backend: `core-sidestate/families/swipe.rs` — `Swipe`. Right is a
/// finger moving left to right.
struct SwipeSetting: Codable, Equatable, Sendable {
    var left: SwipeChoice
    var right: SwipeChoice

    static let standard = SwipeSetting(left: .trash, right: .archive)
}

extension MailrsClient {
    func swipeSetting() async throws -> SwipeSetting { try await getJSON("/api/settings/swipe") }

    func saveSwipeSetting(_ s: SwipeSetting) async throws -> SwipeSetting {
        try await writeJSON("PUT", "/api/settings/swipe", body: s)
    }
}
