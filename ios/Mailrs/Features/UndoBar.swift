import SwiftUI

/// "Archived — Undo", floating over the list.
///
/// Shared, because archiving by swipe with no way back is the same
/// mistake on every screen that offers the swipe — and until now only
/// the phone offered the way back. The iPad and the Mac had the
/// gesture and not the retraction, which is the worse half to have.
///
/// Anchored to the bottom on the phone, where the thumb is. The other
/// two place it themselves; the bar only knows what it says.
struct UndoBar: View {
    @Environment(Session.self) private var session

    var body: some View {
        if session.pendingUndo != nil {
            HStack(spacing: 12) {
                Text(label)
                    .foregroundStyle(.white)
                Button("Undo") {
                    Task { await session.undoArchive() }
                }
                .fontWeight(.semibold)
                .accessibilityIdentifier("undo-archive")
            }
            .font(.subheadline)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(.thinMaterial, in: Capsule())
            .padding(.bottom, 24)
            .transition(.move(edge: .bottom).combined(with: .opacity))
        }
    }

    private var label: LocalizedStringKey {
        let count = session.pendingUndo?.rows.count ?? 1
        if count > 1 { return "Archived ×\(count)" }
        return "Archived"
    }
}
