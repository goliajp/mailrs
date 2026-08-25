import SwiftUI

/// The Mac app.
///
/// A separate target with its own scene, its own shell and its own
/// menus — sharing the model, the wire and the message rendering, and
/// none of the phone's screens. Mac Catalyst would have shipped the
/// iPad layout in a window with a Mac's title bar, which is the thing
/// people mean when they say an app "feels ported": the sidebar is
/// touch-sized, the toolbar is not a toolbar, ⌘ does nothing, and the
/// window opens at a phone's proportions.
///
/// What that costs is this file and `MacRootView`; what it buys is a
/// window that behaves like a window.
@main
struct MailrsMacApp: App {
    @State private var session = Session()
    @State private var preferences = Preferences()
    @State private var senderIcons = SenderIcons()

    var body: some Scene {
        WindowGroup {
            MacRootView()
                .environment(session)
                .environment(preferences)
                .environment(senderIcons)
                .preferredColorScheme(preferences.appearance.colorScheme)
                .environment(\.locale, preferences.language.locale ?? Locale.autoupdatingCurrent)
                .environment(\.timeZone, preferences.timeZone)
                // A mail client is not a utility panel. Wide enough for
                // three columns at their comfortable widths, so the
                // first launch is not a window somebody has to resize
                // before the app makes sense.
                .frame(minWidth: 900, minHeight: 560)
        }
        .defaultSize(width: 1_240, height: 800)
        // The Preferences window, which ⌘, opens. Without a `Settings`
        // scene that shortcut is greyed out and the options have to
        // live inside the content — the phone's answer, because a
        // phone has no second window.
        Settings {
            MacSettingsView()
                .environment(preferences)
                .preferredColorScheme(preferences.appearance.colorScheme)
                .environment(\.locale, preferences.language.locale ?? Locale.autoupdatingCurrent)
        }

        .commands {
            // The menus a Mac app is expected to have. Without these
            // the app has a menu bar that offers nothing but Quit,
            // which is the clearest sign of an iOS app in a window.
            CommandGroup(replacing: .newItem) {
                Button("New Message") { NotificationCenter.default.post(name: .macCompose, object: nil) }
                    .keyboardShortcut("n", modifiers: .command)
            }
            CommandGroup(after: .newItem) {
                Button("Fetch Mail") { NotificationCenter.default.post(name: .macRefresh, object: nil) }
                    .keyboardShortcut("r", modifiers: .command)
                Divider()
                Button("Mark All as Read") {
                    NotificationCenter.default.post(name: .macMarkAllRead, object: nil)
                }
                Divider()
                // **Signing out.** The phone offers it in Settings and
                // the iPad shows the phone's Settings, so both have a
                // way out. This window's Preferences holds appearance
                // and language and nothing else, so until now signing
                // in on a Mac was a door that only opened inwards.
                //
                // In File rather than in the app menu, where several
                // apps put it: under automation the app menu does not
                // open — four ways of driving it timed out waiting for
                // the menu, while File beside it opens every time. A
                // menu item no test can reach is one nobody can prove
                // is wired, and both homes are conventional.
                Button("Sign Out") {
                    NotificationCenter.default.post(name: .macSignOut, object: nil)
                }
            }
        }
    }
}

extension Notification.Name {
    /// Menu commands reach the view through the notification centre
    /// because a `Commands` builder is outside the scene's environment
    /// and cannot hold a reference to what is on screen.
    static let macCompose = Notification.Name("mailrs.mac.compose")
    static let macRefresh = Notification.Name("mailrs.mac.refresh")
    static let macMarkAllRead = Notification.Name("mailrs.mac.markAllRead")
    static let macSignOut = Notification.Name("mailrs.mac.signOut")
}
