import SwiftUI

/// Undoing a send, templates and draft removal: what the composers do
/// beyond sending. Out of `Session+Compose.swift`, which holds the sends.
@MainActor
extension Session {

    /// A send the server is still holding (`undo_seconds`), offered back
    /// for a few seconds the way the web's toast does.
    struct PendingSend: Equatable {
        enum State: Equatable {
            case offered
            case cancelling
            case cancelled
            case tooLate
        }

        let id = UUID()
        let messageId: String
        var state: State
    }

    func offerUndoSend(messageId: String) {
        guard !messageId.isEmpty else { return }
        let offer = PendingSend(messageId: messageId, state: .offered)
        withAnimation { pendingSend = offer }
        Task { [weak self] in
            try? await Task.sleep(for: SendHold.offered)
            guard let self, self.pendingSend?.id == offer.id,
                  self.pendingSend?.state == .offered else { return }
            withAnimation { self.pendingSend = nil }
        }
    }

    /// Stop the held send. 409 means it already left, which the bar says
    /// rather than pretending the Undo worked.
    func undoSend() async {
        guard let client, var pending = pendingSend, pending.state == .offered else { return }
        pending.state = .cancelling
        pendingSend = pending
        var outcome = PendingSend.State.cancelled
        do {
            try await client.cancelPendingSend(messageId: pending.messageId)
        } catch {
            outcome = .tooLate
        }
        guard pendingSend?.id == pending.id else { return }
        pending.state = outcome
        withAnimation { pendingSend = pending }
        let shown = pending.id
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(3))
            guard let self, self.pendingSend?.id == shown else { return }
            withAnimation { self.pendingSend = nil }
        }
    }


    /// The account's templates; an empty list when there are none or the
    /// read failed — the menu then simply is not offered.
    func loadTemplates() async -> [Wire.Template] {
        guard let client else { return [] }
        return (try? await client.templates()) ?? []
    }


    func deleteDraft(id: Int64) async {
        guard let client else { return }
        try? await client.deleteDraft(id: id)
        drafts.removeAll { $0.id == id }
    }
}
