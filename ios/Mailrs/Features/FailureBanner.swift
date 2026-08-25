import SwiftUI

/// A failed request, over the mailbox it happened in.
///
/// `session.banner` is written wherever an operation fails — an archive
/// the server refused, a fetch that could not reach it, a send that did
/// not go. Only the phone was reading it: on the iPad and the Mac those
/// failures happened and **nothing appeared**, so an archive the server
/// refused looked exactly like one that worked.
///
/// Behaviour is the phone's, verbatim, because it shipped that way and
/// a test names it — four seconds, or a tap, whichever comes first.
struct FailureBanner: View {
    @Environment(Session.self) private var session

    var body: some View {
        @Bindable var session = session
        if let banner = session.banner {
            Text(banner)
                .font(.footnote)
                .foregroundStyle(.white)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(Color.red.opacity(0.92), in: Capsule())
                .padding(.top, 6)
                .accessibilityIdentifier("error-banner")
                .onTapGesture { session.banner = nil }
                // Keyed on the message: `.task` alone is tied to the
                // view's identity, which does not change when the string
                // does, so a second error inherited the first one's
                // remaining time and could vanish almost at once.
                .task(id: banner) {
                    try? await Task.sleep(for: .seconds(4))
                    session.banner = nil
                }
                .transition(.move(edge: .top).combined(with: .opacity))
        }
    }
}
