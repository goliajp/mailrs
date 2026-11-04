import SwiftUI
import UserNotifications

/// The conversation a tapped notification asked to open, waiting for the
/// list to show it. Held outside `Session` because the tap can arrive
/// before any screen exists, on a cold launch.
@MainActor
@Observable
final class NotificationRoute {
    static let shared = NotificationRoute()
    var threadId: String?
}

extension Session {
    /// The row a notification is about, after reloading the list so a
    /// message that arrived since the last refresh is in it — Android's
    /// `openThreadById` does the same.
    func conversationForNotification(_ threadId: String) async -> Wire.Conversation? {
        await loadConversations()
        return conversations.first { $0.threadId == threadId }
    }

    /// Archive or mark read from the notification itself, without opening
    /// the app's screens. By thread id: the row may not be loaded.
    func act(onNotification action: PushPayload.Action, threadId: String) async {
        guard let client else { return }
        switch action {
        case .archive:
            try? await client.archive(threadId: threadId)
        case .markRead:
            try? await client.setRead(threadId: threadId, true)
        case .open, .none:
            return
        }
        await refreshBadge()
    }
}

/// The notification category with its two actions, registered at launch.
/// Background actions: neither needs the app on screen.
@MainActor
enum NotificationActions {
    static func register() {
        let archive = UNNotificationAction(
            identifier: PushPayload.archive, title: String(localized: "Archive"), options: []
        )
        let read = UNNotificationAction(
            identifier: PushPayload.markRead, title: String(localized: "Mark as read"), options: []
        )
        let mail = UNNotificationCategory(
            identifier: PushPayload.category, actions: [archive, read],
            intentIdentifiers: [], options: []
        )
        UNUserNotificationCenter.current().setNotificationCategories([mail])
    }
}
