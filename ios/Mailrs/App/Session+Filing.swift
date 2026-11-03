import Foundation
import SwiftUI

/// Filing beyond the inbox: the Trash, Review and labels.
@MainActor
extension Session {

    /// The rows of the list on screen: Review reads its own endpoint,
    /// every other list `/api/conversations` with its axes.
    func fetchPage(_ client: MailrsClient, before: Int64?) async throws -> [Wire.Conversation] {
        if activeList == .review { return try await client.held(before: before) }
        return try await client.conversations(axes: axes, before: before)
    }

    /// The reader's labels, for the sidebar and the label menu.
    func loadLabels() async {
        guard let client else { return }
        if let found = try? await client.labels() { labels = found }
    }

    /// Show one label's conversations.
    ///
    /// Always reloads, unlike `select(_:)`: going from one label to
    /// another stays in `.label`, and the rows still have to change.
    func select(label: Wire.LabelItem) async {
        clearUndo()
        activeLabel = label
        activeList = .label
        conversations = []
        initialLoading = true
        searchResults = []
        searchQuery = nil
        reachedEnd = false
        await loadConversations()
    }

    /// One action from the table on the given rows.
    ///
    /// The ones that take a row out of the list do so at once and offer
    /// the undo; a refusal puts the rows the server kept back where they
    /// were. Deleting for good is the exception: it cannot be undone, so
    /// the rows go only once the server says they are gone, and it asks
    /// for the password when the server wants it.
    func perform(
        _ action: RowAction, on rows: [Wire.Conversation], label: Wire.LabelItem? = nil
    ) async {
        guard let client, !rows.isEmpty else { return }
        let ids = rows.map(\.threadId)
        switch action {
        case .deleteForever:
            await deleteForever(ids, client: client)
        case .release:
            await release(ids, client: client)
        case .label:
            guard let label else { return }
            await setLabel(label, on: ids, put: true, client: client)
        case .unlabel:
            guard let label = label ?? activeLabel else { return }
            await setLabel(label, on: ids, put: false, client: client)
        default:
            guard let verb = action.batch else { return }
            let removed = removeRows(Set(ids))
            offerUndo(removed, action: action)
            await send(verb, ids: ids, putBack: removed, client: client)
        }
    }

    /// The undo of `action`, sent for rows the bar already put back.
    func takeBack(_ rows: [UndoableRow], with action: RowAction, client: MailrsClient) async {
        guard let verb = action.batch else { return }
        let ids = rows.map(\.conversation.threadId)
        do {
            let refused = try await client.batch(action: verb, threadIds: ids)
            if !refused.isEmpty { _ = removeRows(refused) }
        } catch {
            _ = removeRows(Set(ids))
            banner = error.localizedDescription
        }
    }

    private func send(
        _ verb: String, ids: [String], putBack: [UndoableRow], client: MailrsClient
    ) async {
        var failed: [UndoableRow] = []
        do {
            let refused = try await client.batch(action: verb, threadIds: ids)
            failed = putBack.filter { refused.contains($0.conversation.threadId) }
        } catch {
            failed = putBack
            banner = error.localizedDescription
        }
        guard !failed.isEmpty else { return }
        clearUndo()
        withAnimation { conversations = Session.reinserted(failed, into: conversations) }
    }

    private func deleteForever(_ ids: [String], client: MailrsClient) async {
        var gone: Set<String> = []
        do {
            for id in ids {
                try await confirmed { try await client.deleteForever(threadId: id) }
                gone.insert(id)
            }
        } catch MailrsError.confirmRequired {
            // the reader dismissed the password prompt: nothing more goes
        } catch {
            banner = error.localizedDescription
        }
        _ = removeRows(gone)
    }

    private func release(_ ids: [String], client: MailrsClient) async {
        var released: Set<String> = []
        for id in ids {
            do {
                try await client.release(threadId: id)
                released.insert(id)
            } catch {
                banner = error.localizedDescription
            }
        }
        _ = removeRows(released)
    }

    private func setLabel(
        _ label: Wire.LabelItem, on ids: [String], put: Bool, client: MailrsClient
    ) async {
        var verb = "unlabel"
        if put { verb = "label" }
        do {
            let refused = try await client.batch(action: verb, threadIds: ids, labelId: label.id)
            let done = Set(ids).subtracting(refused)
            for id in done {
                patch(id) { row in
                    var now = row.labels ?? []
                    now.removeAll { $0 == label.id }
                    if put { now.append(label.id) }
                    row.labels = now
                }
            }
            // taking the label off the list it names takes the row off it
            if !put, activeList == .label, activeLabel?.id == label.id {
                _ = removeRows(done)
            }
        } catch {
            banner = error.localizedDescription
        }
    }
}
