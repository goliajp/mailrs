import Foundation
import SwiftUI

/// Acting on a selection rather than a row.
///
/// One request for the whole selection, and the answer names the
/// threads it refused — which is what lets these put back exactly the
/// rows that did not go through. The per-row loop they replaced sent
/// fifty requests for fifty rows and could learn only *how many*
/// failed, so a half-failed batch had to roll back everything or
/// nothing.
///
/// Split from `Session+Triage.swift` at the 500-line limit: acting on
/// a selection is its own subject, and it is the one that grew.
@MainActor
extension Session {

    /// Archive a batch, and take the rows off the list.
    ///
    /// Optimistic, because archiving is reversible: refused rows come
    /// back, and the worst case is a row that reappears rather than
    /// mail that is gone. The whole batch shares one undo slot — the
    /// toast the single-row swipe gets, with a count on it.
    func archiveAll(_ selected: [Wire.Conversation]) async {
        guard let client, !selected.isEmpty else { return }
        let ids = Set(selected.map(\.threadId))
        let rows = removeRows(ids)
        // The undo slot opens with the optimistic removal, so the toast
        // is there the moment the rows leave. Archiving from the
        // Archived list is the one place undo would un-archive into the
        // wrong tab, so it stays silent there.
        if activeList != .archived {
            offerUndo(rows)
        }
        // One request for the batch, and the ids it could not do — a
        // row per request was fifty round trips for a fifty-row
        // selection, and the loop existed only because the route used
        // to report a count without saying which ones.
        var failed: [UndoableRow] = []
        do {
            let refused = try await client.batch(
                action: "archive", threadIds: rows.map(\.conversation.threadId))
            failed = rows.filter { refused.contains($0.conversation.threadId) }
        } catch {
            // The request itself did not land, so nothing was archived.
            failed = rows
        }
        if !failed.isEmpty {
            // The rows the server kept come back; the ones it archived
            // stay gone. A live undo toast over a half-failed batch
            // would un-archive rows that were never archived.
            clearUndo()
            withAnimation { conversations = Session.reinserted(failed, into: conversations) }
            banner = "archive failed for \(failed.count) of \(rows.count)"
        }
    }

    /// Mark a batch read. Rows already read are skipped on the wire —
    /// the server call exists to change something.
    func markAllRead(_ selected: [Wire.Conversation]) async {
        guard let client else { return }
        // Only the ones that are unread: the call exists to change
        // something, and a request for a thread already read is a
        // round trip that reports success for work it did not do.
        let targets = selected.filter { $0.unreadCount > 0 }
        guard !targets.isEmpty else { return }
        for conversation in targets {
            withAnimation { patch(conversation.threadId) { $0.unreadCount = 0 } }
        }
        do {
            let refused = try await client.batch(
                action: "read", threadIds: targets.map(\.threadId))
            // Exactly the ones the server refused go back to what they
            // were — a batch that half-worked has to put back its own
            // half, and until the route said which ids failed this
            // client sent one request per row to find out.
            for conversation in targets where refused.contains(conversation.threadId) {
                withAnimation {
                    patch(conversation.threadId) { $0.unreadCount = conversation.unreadCount }
                }
            }
            if !refused.isEmpty {
                banner = String(localized: "\(refused.count) could not be marked read")
            }
        } catch {
            for conversation in targets {
                withAnimation {
                    patch(conversation.threadId) { $0.unreadCount = conversation.unreadCount }
                }
            }
            banner = error.localizedDescription
        }
        await refreshBadge()
    }

    /// Delete a batch. Sequential and non-optimistic for the same
    /// reason the single delete is: rows leave only as the server
    /// confirms each one is gone.
    func deleteAll(_ selected: [Wire.Conversation]) async {
        guard let client, !selected.isEmpty else { return }
        // One request, and rows leave only once the server says they
        // are gone — the same order the single delete uses, because a
        // row that reappears on the next refresh is worse than one
        // that takes a moment to go.
        do {
            let refused = try await client.batch(
                action: "delete", threadIds: selected.map(\.threadId))
            let gone = selected.map(\.threadId).filter { !refused.contains($0) }
            _ = removeRows(Set(gone))
            if !refused.isEmpty {
                banner = String(localized: "\(refused.count) could not be deleted")
            }
        } catch {
            banner = error.localizedDescription
        }
    }
}
