import UserNotifications

/// Push and the icon badge, for the Mac target.
///
/// The iOS target has `App/PushRegistrar.swift`, which is an
/// `UIApplicationDelegate` and an APNs registration — neither of which
/// exists on the Mac in that form. Rather than scatter `#if os(iOS)`
/// through `Session`, the two names it calls are given macOS
/// implementations here and the shared code stays readable.
///
/// **The Mac does not register for remote notifications yet.** Asking
/// for authorisation it will never use would put a permission prompt
/// in front of somebody for a feature that is not there; the badge is
/// real and is set. When Mac push is wired, it goes here.
#if os(macOS)
    enum PushRegistrar {
        static func requestAuthorization() {
            // Deliberately nothing. See the note above.
        }
    }

    enum AppBadge {
        static func update(_ count: Int) {
            UNUserNotificationCenter.current().setBadgeCount(count)
        }
    }
#endif
