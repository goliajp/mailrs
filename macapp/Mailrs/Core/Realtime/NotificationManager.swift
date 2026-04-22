import Foundation
import UserNotifications

#if os(iOS)
import UIKit
#elseif os(macOS)
import AppKit
#endif

/// Local notification + badge helper. Tap routing is wired via the shared
/// selected-thread handler to set MailModel.selectedThreadId.
@MainActor
final class NotificationManager: NSObject, UNUserNotificationCenterDelegate {
    static let shared = NotificationManager()

    /// Handler invoked on notification tap. Set this in MailShellView on task.
    var onSelectThread: ((String) -> Void)?

    func configure() {
        UNUserNotificationCenter.current().delegate = self
    }

    func requestAuthorizationIfNeeded() async -> Bool {
        let center = UNUserNotificationCenter.current()
        let current = await center.notificationSettings()
        switch current.authorizationStatus {
        case .authorized, .provisional, .ephemeral:
            return true
        case .denied:
            return false
        case .notDetermined:
            return (try? await center.requestAuthorization(options: [.alert, .sound, .badge])) ?? false
        @unknown default:
            return false
        }
    }

    func postNewMessage(_ event: NewMessageEvent, withSound: Bool) {
        let content = UNMutableNotificationContent()
        content.title = event.sender
        content.body = event.subject.isEmpty ? event.snippet : event.subject
        content.threadIdentifier = event.thread_id
        content.userInfo = ["thread_id": event.thread_id]
        if withSound { content.sound = .default }

        let request = UNNotificationRequest(
            identifier: "new-message-\(event.thread_id)-\(UUID().uuidString)",
            content: content,
            trigger: UNTimeIntervalNotificationTrigger(timeInterval: 0.1, repeats: false)
        )
        UNUserNotificationCenter.current().add(request)
    }

    func setBadgeCount(_ count: Int) {
        Task {
            try? await UNUserNotificationCenter.current().setBadgeCount(count)
        }
    }

    // MARK: UNUserNotificationCenterDelegate

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        // When app is foreground, only show banners if user explicitly enabled.
        completionHandler([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let userInfo = response.notification.request.content.userInfo
        let threadId = userInfo["thread_id"] as? String
        Task { @MainActor in
            if let threadId { self.onSelectThread?(threadId) }
            completionHandler()
        }
    }
}

extension NotificationManager {
    /// Heuristic for "is the app currently presented to the user". On macOS we
    /// check NSApp.isActive; on iOS we check scene activation state.
    static var isForegroundActive: Bool {
        #if os(iOS)
        guard let scene = UIApplication.shared.connectedScenes
            .compactMap({ $0 as? UIWindowScene })
            .first else { return false }
        return scene.activationState == .foregroundActive
        #elseif os(macOS)
        return NSApplication.shared.isActive
        #else
        return true
        #endif
    }
}
