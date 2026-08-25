import SwiftUI

/// The iPad's first column: which mailbox.
///
/// A source list, which is what this device's own apps use for the
/// same question — and what the phone answers with a drawer, because
/// a phone has nowhere to keep it. Here it stays on screen, so
/// switching mailboxes costs one tap and never hides what is being
/// read.
struct PadSidebar: View {
    @Environment(Session.self) private var session
    @Binding var showingSettings: Bool

    var body: some View {
        List(selection: selectionBinding) {
            Section {
                ForEach(MailList.allCases) { list in
                    SidebarMailboxRow(
                        list: list,
                        unread: list.badgeCount(
                            activeList: session.activeList,
                            unreadInActive: session.unreadInList))
                        .tag(list)
                        .accessibilityIdentifier("pad.list.\(list.rawValue)")
                }
            }

            Section {
                Button {
                    showingSettings = true
                } label: {
                    Label("Settings", systemImage: "gearshape")
                }
                .accessibilityIdentifier("pad.settings")
            }
        }
        .navigationTitle("Mailrs")
        .accessibilityIdentifier("pad.sidebar")
    }

    /// Non-optional selection: a mail app always has a mailbox open,
    /// and an empty first column would leave the other two showing
    /// something that belongs to no list.
    private var selectionBinding: Binding<MailList?> {
        Binding(
            get: { session.activeList },
            set: { chosen in
                guard let chosen, chosen != session.activeList else { return }
                Task { await session.select(chosen) }
            })
    }
}
