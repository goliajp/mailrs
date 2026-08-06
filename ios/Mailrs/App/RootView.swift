import SwiftUI

struct RootView: View {
    @Environment(Session.self) private var session

    var body: some View {
        switch session.state {
        case .signedIn:
            ConversationListView()
        default:
            SignInView()
        }
    }
}
