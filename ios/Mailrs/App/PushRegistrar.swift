import SwiftUI
import UserNotifications

/// Registering this device for push, at the moment it makes sense.
///
/// The permission prompt fires after sign-in, not at launch: an app that
/// asks for notifications before showing any mail is asking to be
/// declined, and iOS only asks once.
///
/// Skipped entirely under the UI-test launch flags. The prompt is a
/// system alert over the whole screen — the same shape as the
/// save-password sheet that made every row untappable — and no test can
/// dismiss it.
@MainActor
enum PushRegistrar {
    static var isUnderTest: Bool {
        let arguments = ProcessInfo.processInfo.arguments
        return arguments.contains("-mailrsSignedOut") || arguments.contains("-mailrsToken")
    }

    static func requestAndRegister() {
        guard !isUnderTest else { return }
        Task {
            let center = UNUserNotificationCenter.current()
            let granted = (try? await center.requestAuthorization(options: [.alert, .badge, .sound]))
                ?? false
            guard granted else { return }
            // Hands the token to `AppDelegate` via the system callback;
            // fails with "no valid aps-environment" until the App ID
            // carries the push capability, which is a portal step.
            UIApplication.shared.registerForRemoteNotifications()
        }
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
