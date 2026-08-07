import SwiftUI

@main
struct MailrsApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var session = Session()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(session)
                .onAppear { AppDelegate.session = session }
        }
    }
}
