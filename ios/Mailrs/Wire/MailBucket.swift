import SwiftUI

/// Where a thread sits once the classifier — or the reader — has had
/// its say.
///
/// The server routes arriving mail into one of these and exposes a verb
/// for each; this is the client's name for them, so a menu and a swipe
/// can offer the same three without spelling the paths twice.
enum MailBucket: String, CaseIterable, Identifiable {
    case inbox
    case notification
    case promotion

    var id: String { rawValue }

    /// The path the server takes.
    var verb: String {
        switch self {
        case .inbox: return "move-to-inbox"
        case .notification: return "mark-notification"
        case .promotion: return "mark-promotion"
        }
    }

    var label: LocalizedStringKey {
        switch self {
        case .inbox: return "Move to Inbox"
        case .notification: return "Mark as notification"
        case .promotion: return "Mark as promotion"
        }
    }

    var systemImage: String {
        switch self {
        case .inbox: return "tray"
        case .notification: return "bell"
        case .promotion: return "tag"
        }
    }

    /// The buckets worth offering while looking at `list` — never the
    /// one the thread is already in.
    static func offered(from list: MailList) -> [MailBucket] {
        switch list {
        case .inbox: return [.notification, .promotion]
        case .np: return [.inbox]
        default: return allCases
        }
    }
}
