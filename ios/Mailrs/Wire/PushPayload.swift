import Foundation

/// What a mail notification carries and offers, shared by the delegate
/// that receives the tap and the tests that fake one.
///
/// The conversation id travels as `thread_id` at the top of the payload,
/// beside `aps`, and the two actions hang off the `MAIL` category — the
/// same pair Android's notification offers (archive, mark read).
enum PushPayload {
    static let category = "MAIL"
    static let archive = "ARCHIVE"
    static let markRead = "MARK_READ"
    static let threadKey = "thread_id"

    /// The conversation a notification is about, or nil when it names none.
    static func threadId(_ userInfo: [AnyHashable: Any]) -> String? {
        guard let raw = userInfo[threadKey] as? String else { return nil }
        let trimmed = raw.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return nil }
        return trimmed
    }

    enum Action: Equatable, Sendable {
        case open
        case archive
        case markRead
        /// Dismissed, or an action this app does not know: nothing to do.
        case none
    }

    static func action(for identifier: String, defaultIdentifier: String, dismissIdentifier: String) -> Action {
        switch identifier {
        case defaultIdentifier: return .open
        case archive: return .archive
        case markRead: return .markRead
        case dismissIdentifier: return .none
        default: return .none
        }
    }
}
