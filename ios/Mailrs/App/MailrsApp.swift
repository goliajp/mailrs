import SwiftUI

@main
struct MailrsApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var session = Session()
    @State private var preferences = Preferences()
    @State private var senderIcons = SenderIcons()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(session)
                .environment(preferences)
                .environment(senderIcons)
                // The three choices that change every screen, applied
                // once here: SwiftUI has an environment slot for each,
                // so no view has to consult a preference to draw.
                .preferredColorScheme(preferences.appearance.colorScheme)
                .environment(\.locale, preferences.language.locale ?? Locale.autoupdatingCurrent)
                .environment(\.timeZone, preferences.timeZone)
                .onAppear { AppDelegate.session = session }
        }
    }
}
