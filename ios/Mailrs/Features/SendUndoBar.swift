import SwiftUI

/// "Message sent — Undo", for the few seconds the server still holds a
/// send (`undo_seconds`). The web offers the same through its toast; here it
/// floats where the archive undo does, and after a tap it says how it went:
/// cancelled, or too late because the message had already left.
struct SendUndoBar: View {
    @Environment(Session.self) private var session

    var body: some View {
        if let pending = session.pendingSend {
            HStack(spacing: 12) {
                Text(SendUndoBar.label(pending.state))
                    .foregroundStyle(.white)
                    .accessibilityIdentifier("send-undo-status")
                if pending.state == .offered {
                    Button("Undo") {
                        Task { await session.undoSend() }
                    }
                    .fontWeight(.semibold)
                    .accessibilityIdentifier("undo-send")
                }
            }
            .font(.subheadline)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(.thinMaterial, in: Capsule())
            .padding(.bottom, 24)
            .transition(.move(edge: .bottom).combined(with: .opacity))
        }
    }

    static func label(_ state: Session.PendingSend.State) -> LocalizedStringKey {
        switch state {
        case .offered:
            return "Message sent"
        case .cancelling:
            return "Cancelling…"
        case .cancelled:
            return "Send cancelled"
        case .tooLate:
            return "Too late to undo — already sent"
        }
    }
}
