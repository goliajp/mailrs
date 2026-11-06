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
    @Binding var showingDrafts: Bool

    var body: some View {
        List(selection: selectionBinding) {
            Section {
                ForEach(MailList.shown) { list in
                    SidebarMailboxRow(
                        list: list,
                        unread: list.badgeCount(
                            activeList: session.activeList,
                            unreadInActive: session.unreadInList))
                        .tag(list)
                        .accessibilityIdentifier("pad.list.\(list.rawValue)")
                }
            }
            if !session.labels.isEmpty {
                Section("Labels") {
                    ForEach(session.labels) { label in
                        Button {
                            Task { await session.select(label: label) }
                        } label: {
                            Label(label.name, systemImage: "tag")
                        }
                        .accessibilityIdentifier("pad.label.\(label.id)")
                    }
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
                // drafts open in their own sheet and the list stays put
                if chosen == .draft {
                    showingDrafts = true
                    return
                }
                Task { await session.select(chosen) }
            })
    }
}
