import SwiftUI
import UserNotifications

/// What this app asks the system for, and what it does not.
///
/// **Only the badge.** The unread count on the icon needs badge
/// authorization and nothing else — no server, no APNs key, no portal
/// step. Alerts and sounds are not requested, because push is not
/// wanted here, and iOS asks once: spending that one prompt on a
/// capability nobody wants and nothing can deliver is how an app gets
/// its notifications declined forever.
///
/// The token plumbing below is left intact and unused. Turning push on
/// later is one call — `registerForRemoteNotifications()` — plus the
/// APNs key and App ID capability, which are portal steps.
///
/// The prompt still waits for sign-in rather than launch, and is
/// skipped under the UI-test flags: it is a system alert over the whole
/// screen, the same shape as the save-password sheet that once made
/// every row untappable, and no test can dismiss it.
@MainActor
enum PushRegistrar {
    static var isUnderTest: Bool {
        let arguments = ProcessInfo.processInfo.arguments
        return arguments.contains("-mailrsSignedOut") || arguments.contains("-mailrsToken")
    }

    static func requestBadgeAuthorization() {
        guard !isUnderTest else { return }
        Task {
            _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.badge])
        }
    }
}

/// The app icon's unread badge.
///
/// Server-counted, never client-derived: the pages in hand are one list
/// and fifty rows of it, and summing them would show a number that
/// changes with scrolling. `setBadgeCount` silently does nothing until
/// badge authorization is granted, which is the one thing the sign-in
/// prompt asks for — so this needs no gating of its own.
@MainActor
enum AppBadge {
    static func update(_ count: Int) {
        guard !PushRegistrar.isUnderTest else { return }
        UNUserNotificationCenter.current().setBadgeCount(count) { _ in }
    }
}

/// The UIKit half: token callbacks have no SwiftUI surface.
final class AppDelegate: NSObject, UIApplicationDelegate {
    /// Set by `MailrsApp` so the token can reach the session's client.
    static weak var session: Session?

    func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        // The wire format is lowercase hex — the same string Apple's
        // gateway expects in the request path.
        let token = deviceToken.map { String(format: "%02x", $0) }.joined()
        Task { @MainActor in
            await AppDelegate.session?.registerPushToken(token)
        }
    }

    func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        // Expected on devices until the App ID has the push capability,
        // and on simulators without a paired developer account. Logged,
        // not surfaced: the user cannot act on it.
        print("push registration unavailable: \(error.localizedDescription)")
    }
}
