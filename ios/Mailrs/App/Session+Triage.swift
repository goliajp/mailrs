import Foundation
import SwiftUI

/// Acting on threads: read, starred, junk, archive, delete, undo.
///
/// Split out of `Session.swift` at the 500-line limit this
/// repository holds every language to — and which did not look at
/// `ios/` until now. One type, several subjects.
@MainActor
extension Session {

    /// A thread is read because someone is reading it.
    ///
    /// Called by the thread view once its messages are on screen — not
    /// by the list, and not on selection. The web client learned that
    /// distinction the hard way: a hidden pane auto-opening the newest
    /// thread marked mail read that had never been displayed. Here the
    /// view only exists when a person navigated into it, which is what
    /// makes this safe.
    func markThreadRead(_ conversation: Wire.Conversation) async {
        guard conversation.unreadCount > 0, let client else { return }
        do {
            try await client.setRead(threadId: conversation.threadId, true)
            // Patched after the server confirms, and the row is not
            // re-filtered: in the Unread list the row stays visible
            // until the next refresh rather than vanishing while you
            // are standing on it.
            withAnimation { patch(conversation.threadId) { $0.unreadCount = 0 } }
            await refreshBadge()
        } catch {
            // Still unread is the honest state if the call failed; the
            // next open retries by construction.
        }
    }


    /// Toggle read, and show it immediately.
    ///
    /// Optimistic, like archive and unlike delete: both directions are
    /// reversible by the same gesture, so the worst a failed call costs
    /// is a row that snaps back.
    func toggleRead(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        let markRead = conversation.unreadCount > 0
        let previous = conversations
        withAnimation {
            patch(conversation.threadId) { row in
                if markRead {
                    row.unreadCount = 0
                    return
                }
                row.unreadCount = max(1, row.unreadCount)
            }
        }
        do {
            try await client.setRead(threadId: conversation.threadId, markRead)
        } catch {
            conversations = previous
            banner = error.localizedDescription
        }
    }


    func toggleStarred(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        let starred = !conversation.flagged
        let previous = conversations
        withAnimation { patch(conversation.threadId) { $0.flagged = starred } }
        do {
            try await client.setStarred(threadId: conversation.threadId, starred)
        } catch {
            conversations = previous
            banner = error.localizedDescription
        }
    }


    /// Pin, or unpin.
    ///
    /// Optimistic like the star, and it moves the row: `PinOrder`
    /// arranges at render, so the patch is what lifts it.
    func togglePinned(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        let pinned = !conversation.pinned
        let previous = conversations
        withAnimation { patch(conversation.threadId) { $0.pinned = pinned } }
        do {
            try await client.setPinned(threadId: conversation.threadId, pinned)
        } catch {
            conversations = previous
            banner = error.localizedDescription
        }
    }


    /// Replace one row in whichever collection is on screen.
    ///
    /// Take rows off **both** stores.
    ///
    /// `conversations` and `searchResults` hold the same rows twice —
    /// the web has one array and cannot have this bug, which is the
    /// same argument `frontend/no-rq-mirror` makes about mirroring a
    /// query into state. Until there is one store here, every removal
    /// has to say so twice, and archive and delete only ever said it
    /// once: archiving a thread while a search was on screen left the
    /// row sitting in the results, and tapping it opened a thread that
    /// was no longer in the list behind it.
    ///
    /// Returns what left the list, with the positions undo needs.
    // `internal`, not `private`: the selection actions live in
    // `Session+Selection.swift` and Swift has no visibility that means
    // "this type, across its files". Module scope is the smallest
    // step that compiles.
    func removeRows(_ ids: Set<String>) -> [UndoableRow] {
        let removed = conversations.enumerated()
            .filter { ids.contains($0.element.threadId) }
            .map { UndoableRow(conversation: $0.element, index: $0.offset) }
        withAnimation {
            conversations.removeAll { ids.contains($0.threadId) }
            searchResults.removeAll { ids.contains($0.threadId) }
        }
        return removed
    }


    /// Both, not whichever is showing: a row can be in the list and in
    /// the search results at once, and patching only the visible one
    /// leaves the other holding the old value for when the search is
    /// dismissed.
    // `internal`, not `private`: the selection actions live in
    // `Session+Selection.swift` and Swift has no visibility that means
    // "this type, across its files". Module scope is the smallest
    // step that compiles.
    func patch(_ threadId: String, _ change: (inout Wire.Conversation) -> Void) {
        if let index = conversations.firstIndex(where: { $0.threadId == threadId }) {
            change(&conversations[index])
        }
        if let index = searchResults.firstIndex(where: { $0.threadId == threadId }) {
            change(&searchResults[index])
        }
    }


    /// Mark junk (or rescue from Junk), and take the row off this list.
    ///
    /// Optimistic like archive: the same verb in the other direction
    /// undoes it, and the worst a failure costs is a row that returns.
    /// The row leaves whichever list is showing because the verdict
    /// moves the thread between folders — a spam row lingering in the
    /// Inbox after being marked is the confusing outcome.
    /// Put a thread back to unread — the "deal with this later" verdict.
    ///
    /// Not `toggleRead`: that reads the row's current count to decide,
    /// and this is called from a thread that has just been opened, so
    /// the row already says read. The intent here is one direction, and
    /// a toggle would silently do nothing.
    func markUnread(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        let previous = conversations
        withAnimation { patch(conversation.threadId) { $0.unreadCount = max(1, $0.unreadCount) } }
        do {
            try await client.setRead(threadId: conversation.threadId, false)
        } catch {
            conversations = previous
            banner = error.localizedDescription
        }
        await refreshBadge()
    }


    /// Ask the server to leave the list this message came from.
    ///
    /// Returns whether it worked, rather than setting `state = .failed`:
    /// the answer belongs under the message it is about, not in a
    /// banner over the whole mailbox, and the reader still has the
    /// sender's own link if this comes back false.
    func unsubscribe(threadId: String, uid: UInt32) async -> Bool {
        guard let client else { return false }
        do {
            return try await client.unsubscribe(threadId: threadId, uid: uid)
        } catch {
            return false
        }
    }


    /// Move a thread to another bucket, and take the row off this list.
    ///
    /// Optimistic like junk is, and for the same reason: the row is
    /// leaving a list it no longer belongs to, and a refusal puts it
    /// back.
    func move(_ conversation: Wire.Conversation, to bucket: MailBucket) async {
        guard let client else { return }
        let previous = conversations
        let previousResults = searchResults
        _ = removeRows([conversation.threadId])
        do {
            try await client.moveTo(threadId: conversation.threadId, bucket: bucket)
        } catch {
            withAnimation {
                conversations = previous
                searchResults = previousResults
            }
            banner = error.localizedDescription
        }
    }


    func setJunk(_ conversation: Wire.Conversation, junk: Bool) async {
        guard let client else { return }
        let previous = conversations
        let previousResults = searchResults
        withAnimation {
            conversations.removeAll { $0.threadId == conversation.threadId }
            searchResults.removeAll { $0.threadId == conversation.threadId }
        }
        do {
            try await client.setJunk(threadId: conversation.threadId, junk)
        } catch {
            withAnimation {
                conversations = previous
                searchResults = previousResults
            }
            banner = error.localizedDescription
        }
    }


    /// Archive, and take the row off the list.
    ///
    /// Optimistic, because archiving is reversible: if the server refuses
    /// the row comes back, and the worst case is a row that reappears
    /// rather than mail that is gone.
    func archive(_ conversation: Wire.Conversation) async {
        await archiveAll([conversation])
    }






    /// The archive that can still be taken back. A single slot, as in
    /// Gmail's snackbar: a second archive replaces the first, because a
    /// stack of undos is a history feature, not a safety net. The slot
    /// holds a whole batch — one gesture, one undo.
    struct UndoableRow: Equatable {
        let conversation: Wire.Conversation
        let index: Int
    }


    struct PendingUndo: Equatable {
        let rows: [UndoableRow]
    }


    /// Visible long enough to read and act, short enough that the list
    /// is not wearing a permanent banner.
    static let undoWindow: Duration = .seconds(5)


    // internal for the same reason as `removeRows` above.
    func offerUndo(_ rows: [UndoableRow]) {
        undoDismissTask?.cancel()
        withAnimation { pendingUndo = PendingUndo(rows: rows) }
        let offered = pendingUndo
        undoDismissTask = Task { [weak self] in
            try? await Task.sleep(for: Session.undoWindow)
            guard !Task.isCancelled, let self, self.pendingUndo == offered else { return }
            withAnimation { self.pendingUndo = nil }
        }
    }


    func clearUndo() {
        undoDismissTask?.cancel()
        withAnimation { pendingUndo = nil }
    }


    /// Rows going back to the exact positions they left. Ascending
    /// order is what makes the indices mean what they meant: each
    /// insert restores the coordinate system the next one was recorded
    /// in.
    nonisolated static func reinserted(
        _ rows: [UndoableRow], into list: [Wire.Conversation]
    ) -> [Wire.Conversation] {
        var out = list
        for row in rows.sorted(by: { $0.index < $1.index }) {
            out.insert(row.conversation, at: min(row.index, out.count))
        }
        return out
    }


    /// Take the archive back: every row returns to the position it
    /// left, optimistically — unarchiving is as reversible as
    /// archiving, so the same contract holds in both directions.
    func undoArchive() async {
        guard let client, let undo = pendingUndo else { return }
        clearUndo()
        withAnimation { conversations = Session.reinserted(undo.rows, into: conversations) }
        for row in undo.rows {
            do {
                try await client.unarchive(threadId: row.conversation.threadId)
            } catch {
                withAnimation {
                    conversations.removeAll { $0.threadId == row.conversation.threadId }
                }
                banner = error.localizedDescription
            }
        }
    }


    /// Delete, and take the row off the list.
    ///
    /// Not optimistic. The server unlinks the maildir files, so there is
    /// nothing to restore and no honest way to put the row back — the
    /// row goes only once the server says it is gone.
    func delete(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        do {
            try await client.delete(threadId: conversation.threadId)
            _ = removeRows([conversation.threadId])
        } catch {
            banner = error.localizedDescription
        }
    }


}


/// Putting a conversation away until later.
@MainActor
extension Session {
    /// Snooze, or wake.
    ///
    /// The row leaves the list at once, because on the server it
    /// leaves too: a snooze files the thread away and records when it
    /// is due back. Before v2.55 the field was written to the shared
    /// thread row and read by nobody, so both clients dropped the row
    /// optimistically and the next refresh brought it back.
    func snooze(_ conversation: Wire.Conversation, until: Int64?) async {
        guard let client else { return }
        let previous = conversations
        let previousResults = searchResults
        // Off both stores: `conversations` and `searchResults` hold
        // the same rows twice, and every removal that said it once
        // left the row sitting in whichever list was not on screen.
        withAnimation {
            conversations.removeAll { $0.threadId == conversation.threadId }
            searchResults.removeAll { $0.threadId == conversation.threadId }
        }
        do {
            try await client.setSnoozed(threadId: conversation.threadId, until: until)
        } catch {
            conversations = previous
            searchResults = previousResults
            banner = error.localizedDescription
        }
    }
}


/// The two lists that decide what bypasses the filter, and what never
/// arrives.
@MainActor
extension Session {
    /// Put a sender on one of them.
    ///
    /// Adding is all this offers: the Settings screen is where the
    /// lists can be *read*, and a menu that adds without showing what
    /// is already there is how a list nobody can see grows forever —
    /// which is exactly what the whitelist did for months.
    func addSender(_ address: String, to kind: SenderListKind) async {
        guard let client else { return }
        do {
            try await client.addToSenderList(kind, address: address)
            banner = kind == .blocked
                ? String(localized: "Blocked \(address)")
                : String(localized: "Always allowing \(address)")
        } catch {
            banner = error.localizedDescription
        }
    }
}


/// The list on screen, in one request.
@MainActor
extension Session {
    /// Mark read everything this list is showing.
    ///
    /// One call, not a loop, and scoped by the list's own axes: the
    /// batch bar's per-thread version is right for the rows someone
    /// selected and wrong for a list of 1,458. The list is re-read
    /// afterwards rather than patched — the server flipped rows this
    /// client has never loaded, and patching only what is on screen
    /// would leave the badge and the rows below disagreeing.
    func markListRead() async {
        guard let client else { return }
        do {
            let flipped = try await client.markListRead(axes: axes)
            banner = flipped == 0
                ? String(localized: "Nothing was unread")
                : String(localized: "Marked \(flipped) as read")
            await loadConversations()
            await refreshBadge()
        } catch {
            banner = error.localizedDescription
        }
    }
}
