import SwiftUI

/// What a long press on one row offers.
///
/// Built from the action table (`Wire/RowActions.swift`), the same one
/// the web and Android read, so the actions, their order and their
/// words match on every surface. Only deleting for good asks first; it
/// hands the row to `onDeleteForever`, which owns the question.
struct ConversationRowMenu: View {
    let conversation: Wire.Conversation
    var onDeleteForever: (Wire.Conversation) -> Void = { _ in }
    @Environment(Session.self) private var session

    var body: some View {
        ForEach(actions, id: \.self) { action in
            item(action)
        }
    }

    private var actions: [RowAction] {
        RowActions.forRow(
            conversation,
            list: session.activeList,
            inLabel: session.activeList == .label,
            now: Int64(Date().timeIntervalSince1970))
    }

    @ViewBuilder private func item(_ action: RowAction) -> some View {
        switch action {
        case .read, .unread:
            Button {
                Task { await session.toggleRead(conversation) }
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
        case .star, .unstar:
            Button {
                Task { await session.toggleStarred(conversation) }
            } label: {
                Label(
                    StarToggle.label(starred: conversation.flagged),
                    systemImage: StarToggle.icon(starred: conversation.flagged))
            }
            .accessibilityIdentifier("row.menu.star")
        case .pin, .unpin:
            Button {
                Task { await session.togglePinned(conversation) }
            } label: {
                Label(
                    PinLabel.title(pinned: conversation.pinned),
                    systemImage: PinLabel.icon(pinned: conversation.pinned))
            }
        case .archive:
            Button {
                Task { await session.archive(conversation) }
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
        case .snooze:
            // one idea with four answers
            Menu {
                ForEach(SnoozeChoice.allCases) { choice in
                    Button(choice.label) {
                        Task {
                            await session.snooze(
                                conversation,
                                until: choice.fireDate(after: Date(), calendar: .current))
                        }
                    }
                }
            } label: {
                Label("Snooze", systemImage: "clock")
            }
        case .unsnooze:
            Button {
                Task { await session.snooze(conversation, until: nil) }
            } label: {
                Label("Wake now", systemImage: "bell")
            }
        case .markJunk:
            Button(role: .destructive) {
                Task { await session.setJunk(conversation, junk: true) }
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
        case .markNotJunk:
            Button {
                Task { await session.setJunk(conversation, junk: false) }
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
        case .markNotification, .markPromotion, .moveToInbox:
            bucketButton(action)
        case .label:
            labelMenu
        case .deleteForever:
            Button(role: .destructive) {
                onDeleteForever(conversation)
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
            .accessibilityIdentifier("row.menu.deleteForever")
        default:
            Button(role: role(action)) {
                Task { await session.perform(action, on: [conversation]) }
            } label: {
                Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
            }
            .accessibilityIdentifier("row.menu.\(action.rawValue)")
        }
    }

    private func bucketButton(_ action: RowAction) -> some View {
        Button {
            Task { await session.move(conversation, to: bucket(action)) }
        } label: {
            Label(LocalizedStringKey(action.titleKey), systemImage: action.systemImage)
        }
    }

    /// Every label, ticked where the conversation carries it; choosing
    /// one puts it on or takes it off.
    private var labelMenu: some View {
        Menu {
            ForEach(session.labels) { label in
                let on = (conversation.labels ?? []).contains(label.id)
                Button {
                    Task {
                        await session.perform(
                            labelAction(on), on: [conversation], label: label)
                    }
                } label: {
                    Label(label.name, systemImage: tick(on))
                }
                .accessibilityIdentifier("row.menu.label.\(label.id)")
            }
        } label: {
            Label(LocalizedStringKey(RowAction.label.titleKey), systemImage: "tag")
        }
        .accessibilityIdentifier("row.menu.label")
    }

    private func bucket(_ action: RowAction) -> MailBucket {
        switch action {
        case .markNotification: return .notification
        case .markPromotion: return .promotion
        default: return .inbox
        }
    }

    private func labelAction(_ on: Bool) -> RowAction {
        if on { return .unlabel }
        return .label
    }

    private func tick(_ on: Bool) -> String {
        if on { return "checkmark" }
        return "tag"
    }

    private func role(_ action: RowAction) -> ButtonRole? {
        if action == .trash { return .destructive }
        return nil
    }
}

/// The trailing swipe: archive owns the full swipe everywhere but the
/// Trash, where restoring does; moving to the Trash sits behind it and
/// no longer asks, because the Trash is the way back.
struct TrailingSwipes: View {
    let conversation: Wire.Conversation
    let onDeleteForever: (Wire.Conversation) -> Void
    @Environment(Session.self) private var session

    var body: some View {
        if session.activeList == .trash || conversation.isTrashed {
            Button {
                Task { await session.perform(.untrash, on: [conversation]) }
            } label: {
                Label("Restore", systemImage: RowAction.untrash.systemImage)
            }
            .tint(.green)
            Button(role: .destructive) {
                onDeleteForever(conversation)
            } label: {
                Label("Delete forever", systemImage: RowAction.deleteForever.systemImage)
            }
        } else {
            Button {
                Task { await session.archive(conversation) }
            } label: {
                Label("Archive", systemImage: "archivebox")
            }
            .tint(.green)
            Button(role: .destructive) {
                Task { await session.perform(.trash, on: [conversation]) }
            } label: {
                Label("Move to Trash", systemImage: "trash")
            }
        }
    }
}
