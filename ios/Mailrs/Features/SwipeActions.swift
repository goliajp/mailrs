import SwiftUI

/// The swipe on a row, from the reader's swipe setting (`Session.swipe`),
/// the same one the web and Android read: by default right archives and
/// left moves to the Trash, each with the undo bar. The first button owns
/// the full swipe; the ones behind it are what a short swipe reveals.
///
/// In the Trash both directions restore. Deleting for good is only ever
/// a button behind the full swipe, and it asks first.
struct TrailingSwipes: View {
    let conversation: Wire.Conversation
    let onDeleteForever: (Wire.Conversation) -> Void
    @Environment(Session.self) private var session

    var body: some View {
        let primary = session.swipe.left.action(for: conversation, in: session.activeList)
        if let primary {
            SwipeButton(action: primary, conversation: conversation, onDeleteForever: onDeleteForever)
        }
        ForEach(extras(primary), id: \.self) { action in
            SwipeButton(action: action, conversation: conversation, onDeleteForever: onDeleteForever)
        }
    }

    /// Archive and trash stay a short swipe away whatever the setting says,
    /// and in the Trash restoring and deleting for good do.
    private func extras(_ primary: RowAction?) -> [RowAction] {
        let list = session.activeList
        var out = [SwipeChoice.trash, .archive].compactMap { $0.action(for: conversation, in: list) }
        if conversation.isTrashed || list == .trash { out.append(.deleteForever) }
        var seen: Set<RowAction> = []
        if let primary { seen.insert(primary) }
        return out.filter { seen.insert($0).inserted }
    }
}

/// The leading swipe (a finger moving right): the setting's action, then
/// read or unread, moving to another bucket and the star.
struct LeadingSwipes: View {
    let conversation: Wire.Conversation
    /// Opens the bucket picker; none where the screen has no picker.
    var onMove: ((Wire.Conversation) -> Void)?
    @Environment(Session.self) private var session

    var body: some View {
        let primary = session.swipe.right.action(for: conversation, in: session.activeList)
        if let primary {
            SwipeButton(action: primary, conversation: conversation, onDeleteForever: { _ in })
        }
        if primary != .read && primary != .unread {
            let read = SwipeChoice.read.action(for: conversation, in: session.activeList) ?? .read
            SwipeButton(action: read, conversation: conversation, onDeleteForever: { _ in })
        }
        if let onMove, !MailBucket.offered(from: session.activeList).isEmpty {
            Button {
                onMove(conversation)
            } label: {
                Label("Move", systemImage: "tray.and.arrow.down")
            }
            .tint(.indigo)
        }
        Button {
            Task { await session.toggleStarred(conversation) }
        } label: {
            Label(
                StarToggle.label(starred: conversation.flagged),
                systemImage: StarToggle.icon(starred: conversation.flagged)
            )
        }
        .tint(.yellow)
    }
}

/// One action on a swipe, run the way the row menu runs it.
private struct SwipeButton: View {
    let action: RowAction
    let conversation: Wire.Conversation
    let onDeleteForever: (Wire.Conversation) -> Void
    @Environment(Session.self) private var session

    var body: some View {
        switch action {
        case .read, .unread:
            Button {
                Task { await session.toggleRead(conversation) }
            } label: {
                Label(ReadToggle.label(unread: conversation.unreadCount > 0),
                      systemImage: action.systemImage)
            }
            .tint(.blue)
        case .trash:
            Button(role: .destructive) {
                Task { await session.perform(.trash, on: [conversation]) }
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
        case .deleteForever:
            Button(role: .destructive) {
                onDeleteForever(conversation)
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
        case .archive:
            Button {
                Task { await session.archive(conversation) }
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
            .tint(.green)
        default:
            Button {
                Task { await session.perform(action, on: [conversation]) }
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
            .tint(.green)
        }
    }
}
