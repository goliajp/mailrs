import SwiftUI

/// What the reader has on their two sender lists, lowercased. An address
/// is on one at most: the server takes it off one when it goes on the
/// other, and this copy does the same.
struct SenderLists: Equatable {
    var blocked: Set<String> = []
    var allowed: Set<String> = []

    mutating func block(_ address: String) {
        allowed.remove(address)
        blocked.insert(address)
    }

    mutating func allow(_ address: String) {
        blocked.remove(address)
        allowed.insert(address)
    }
}

/// A block the undo bar can take back.
struct BlockedSender: Equatable {
    let threadId: String
    let address: String
    /// It came off the whitelist, and goes back on with the undo.
    let wasAllowed: Bool
}

/// Blocking the sender of a message, from that message.
@MainActor
extension Session {
    /// Read both lists, for the menu to know whether to offer Block or Unblock.
    func loadSenderLists() async {
        guard let client else { return }
        do {
            let blocked = try await client.senderList(.blocked)
            let allowed = try await client.senderList(.allowed)
            senderLists = SenderLists(
                blocked: Set(blocked.map { $0.lowercased() }),
                allowed: Set(allowed.map { $0.lowercased() })
            )
        } catch {
            banner = error.localizedDescription
        }
    }

    /// Whether `address` is one of the reader's own: blocking it would
    /// send their own replies to Junk, so the menu does not offer it.
    func isOwnAddress(_ address: String) -> Bool {
        let addr = address.lowercased()
        if addr == myAddress { return true }
        return myAliases.contains { $0.sourceAddress.lowercased() == addr }
    }

    /// Blacklist the sender and move the conversation to Junk; the row
    /// leaves the list (unless the list is Junk) and the undo bar can
    /// take all of it back.
    func blockSender(_ address: String, threadId: String) async {
        guard let client else { return }
        let addr = address.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        do {
            let answer = try await client.blockSender(threadId: threadId, address: addr)
            senderLists?.block(answer.address)
            var removed: [UndoableRow] = []
            if activeList != .junk { removed = removeRows([threadId]) }
            offerUndo(
                removed, action: .markJunk,
                blocked: BlockedSender(threadId: threadId, address: answer.address, wasAllowed: answer.wasAllowed)
            )
        } catch {
            banner = error.localizedDescription
        }
    }

    /// Off the blacklist; the conversation stays where it is.
    func unblockSender(_ address: String) async {
        guard let client else { return }
        let addr = address.lowercased()
        do {
            try await client.removeFromSenderList(.blocked, address: addr)
            senderLists?.blocked.remove(addr)
            banner = AppText.string("Unblocked \(addr)")
        } catch {
            banner = error.localizedDescription
        }
    }

    /// The undo of `blockSender`: off the blacklist, back on the
    /// whitelist if it came from there, and the conversation back to the
    /// Inbox. The rows are already back on screen.
    func unblockSender(_ blocked: BlockedSender, client: MailrsClient) async {
        do {
            try await client.unblockSender(
                threadId: blocked.threadId, address: blocked.address, allow: blocked.wasAllowed
            )
            senderLists?.blocked.remove(blocked.address)
            if blocked.wasAllowed { senderLists?.allowed.insert(blocked.address) }
            // back in the Inbox, so out of the Junk list it was blocked from
            if activeList == .junk { _ = removeRows([blocked.threadId]) }
        } catch {
            if activeList != .junk { _ = removeRows([blocked.threadId]) }
            banner = error.localizedDescription
        }
    }
}
