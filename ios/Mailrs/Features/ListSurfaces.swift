import SwiftUI

/// What floats over the phone's conversation list and what opens on top of
/// it from outside: the failure banner, "Message sent — Undo", and the
/// conversation a tapped notification asked for.
///
/// Kept together and out of `ConversationListView`, which is the list.
struct ListSurfaces: ViewModifier {
    @Environment(Session.self) private var session
    /// The conversation a tapped notification asked for, once found.
    @State private var notified: Wire.Conversation?

    func body(content: Content) -> some View {
        content
            .safeAreaInset(edge: .top) {
                if session.activeList == .inbox { InboxSectionBar() }
            }
            .overlay(alignment: .top) { FailureBanner() }
            .animation(.easeOut(duration: 0.2), value: session.banner)
            .overlay(alignment: .bottom) { SendUndoBar().padding(.bottom, 48) }
            .animation(.easeOut(duration: 0.2), value: session.pendingSend)
            .navigationDestination(isPresented: shown) {
                if let notified { ThreadView(conversation: notified) }
            }
            .task(id: NotificationRoute.shared.threadId) { await openNotified() }
            .sheet(isPresented: calendarShown) { CalendarWeekView() }
    }

    private var calendarShown: Binding<Bool> {
        Binding(get: { session.showingCalendar }, set: { session.showingCalendar = $0 })
    }

    private var shown: Binding<Bool> {
        Binding(get: { notified != nil }, set: { isShown in if !isShown { notified = nil } })
    }

    /// Open what a tapped notification named, then forget the request so
    /// coming back to the list does not open it again.
    private func openNotified() async {
        guard let threadId = NotificationRoute.shared.threadId else { return }
        NotificationRoute.shared.threadId = nil
        notified = await session.conversationForNotification(threadId)
    }
}
