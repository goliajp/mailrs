import SwiftUI

struct RootView: View {
    @Environment(Session.self) private var session
    @Environment(\.scenePhase) private var scenePhase
    /// The launch's own `.active` is not a return: `restore()` has
    /// just fetched, and refreshing on top of it doubles every cold
    /// start's traffic.
    @State private var hasBeenActive = false

    var body: some View {
        Group {
            switch session.state {
            case .signedIn:
                ConversationListView()
            default:
                SignInView()
            }
        }
        .task { await session.restore() }
        .onChange(of: scenePhase) { _, phase in
            guard phase == .active else { return }
            guard hasBeenActive else {
                hasBeenActive = true
                return
            }
            // Coming back to the app showed the mailbox as it was when
            // you left it — until push is live, this is the only thing
            // that makes a return show new mail.
            Task { await session.refreshForeground() }
        }
    }
}
