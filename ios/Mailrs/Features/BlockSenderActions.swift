import SwiftUI

/// "Block x@y" or "Unblock x@y", in a message's menus.
///
/// Nothing until both sender lists are read: before then neither word is
/// known to be the right one. Never for the reader's own addresses, whose
/// mail would go to Junk with the sender's.
struct BlockSenderItem: View {
    @Environment(Session.self) private var session
    let address: String
    /// Asks first; the card holds the question, because a menu cannot.
    let onBlock: () -> Void

    var body: some View {
        let addr = address.lowercased()
        if let lists = session.senderLists, !addr.isEmpty, !session.isOwnAddress(addr) {
            if lists.blocked.contains(addr) {
                Button {
                    Task { await session.unblockSender(addr) }
                } label: {
                    Label("Unblock \(addr)", systemImage: "hand.raised")
                }
                .accessibilityIdentifier("sender.unblock")
            } else {
                Button(role: .destructive, action: onBlock) {
                    Label("Block \(addr)", systemImage: "hand.raised")
                }
                .accessibilityIdentifier("sender.block")
            }
        }
    }
}

/// The question before a block: whom, what happens to their mail, where
/// it is undone, and that it comes off the allowed list if it is there.
struct BlockSenderConfirm: ViewModifier {
    @Environment(Session.self) private var session
    @Binding var address: String?
    let threadId: String
    /// Leaves the thread once it is in Junk, as marking it junk does.
    let onBlocked: () -> Void

    private var shown: Binding<Bool> {
        Binding(get: { address != nil }, set: { if !$0 { address = nil } })
    }

    func body(content: Content) -> some View {
        content.alert(
            Text("Block \(address ?? "")?"), isPresented: shown, presenting: address
        ) { addr in
            Button("Block", role: .destructive) {
                Task { await session.blockSender(addr, threadId: threadId) }
                if session.activeList != .junk { onBlocked() }
            }
            .accessibilityIdentifier("sender.block.confirm")
            Button("Cancel", role: .cancel) {}
        } message: { addr in
            Text(BlockSenderConfirm.message(addr, allowed: session.senderLists?.allowed.contains(addr) ?? false))
        }
    }

    static func message(_ address: String, allowed: Bool) -> String {
        let what = AppText.string("Mail from this address will go to Junk, and this conversation moves there now. You can undo this in Settings → Senders.")
        guard allowed else { return what }
        return what + "\n\n" + AppText.string("\(address) is on your Always allowed list. Blocking takes it off.")
    }
}
