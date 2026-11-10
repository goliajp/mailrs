import SwiftUI

/// "Archived — Undo", floating over the list.
///
/// Shared, because archiving by swipe with no way back is the same
/// mistake on every screen that offers the swipe — and until now only
/// the phone offered the way back. The iPad and the Mac had the
/// gesture and not the retraction, which is the worse half to have.
///
/// The label takes the material's own colour: forced white, it vanished
/// on the light glass over a white list.
struct UndoBar: View {
    @Environment(Session.self) private var session

    var body: some View {
        if session.pendingUndo != nil {
            HStack(spacing: 12) {
                Text(label)
                Button("Undo") {
                    Task { await session.undoArchive() }
                }
                .fontWeight(.semibold)
                .accessibilityIdentifier("undo-archive")
            }
            .font(.subheadline)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .floatingGlass(in: .capsule)
            .transition(.move(edge: .bottom).combined(with: .opacity))
        }
    }

    private var label: LocalizedStringKey { UndoBar.label(session.pendingUndo) }

    /// What was done, in the words of the action table.
    static func label(_ undo: Session.PendingUndo?) -> LocalizedStringKey {
        if let blocked = undo?.blocked { return "Blocked \(blocked.address)" }
        let count = undo?.rows.count ?? 1
        let action = undo?.action ?? .archive
        if action != .archive {
            return LocalizedStringKey(action.doneKey ?? action.titleKey)
        }
        if count > 1 { return "Archived ×\(count)" }
        return "Archived"
    }
}

/// The two undo bars, stacked: a send undone and an archive undone can be
/// on offer at once, and placed separately the second covered the first.
struct UndoBars: View {
    var body: some View {
        VStack(spacing: 8) {
            SendUndoBar()
            UndoBar()
        }
    }
}
