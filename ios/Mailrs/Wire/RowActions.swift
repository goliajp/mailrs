import Foundation

/// What can be done to a conversation, defined once.
///
/// The same table as the web's `web/src/lib/actions.ts` and Android's
/// `wire/RowActions.kt`: each action's words, what it is called once
/// done, the action that takes it back, and whether it asks first. Only
/// deleting for good asks — everything else can be undone, and the undo
/// is the protection.
///
/// `batch` is the verb `POST /api/conversations/batch` knows, for the
/// actions it carries; the others have a route of their own.
enum RowAction: String, CaseIterable, Sendable {
    case archive, unarchive, trash, untrash, deleteForever
    case markJunk, markNotJunk, markNotification, markPromotion, moveToInbox
    case pin, unpin, snooze, unsnooze, read, unread, star, unstar
    case label, unlabel
    /// Out of Review: it was not fraud.
    case release

    /// The catalog key of its name.
    var titleKey: String {
        switch self {
        case .archive: "Archive"
        case .unarchive: "Unarchive"
        case .trash: "Move to Trash"
        case .untrash: "Restore"
        case .deleteForever: "Delete forever"
        case .markJunk: "Mark as junk"
        case .markNotJunk: "Not junk"
        case .markNotification: "Mark as Notification"
        case .markPromotion: "Mark as Promotion"
        case .moveToInbox: "Move to Inbox"
        case .pin: "Pin"
        case .unpin: "Unpin"
        case .snooze: "Snooze"
        case .unsnooze: "Wake now"
        case .read: "Mark as read"
        case .unread: "Mark as unread"
        case .star: "Star"
        case .unstar: "Unstar"
        case .label: "Labels"
        case .unlabel: "Remove label"
        case .release: "This is not fraud"
        }
    }

    /// What the undo bar says once it is done, when it offers an undo.
    var doneKey: String? {
        switch self {
        case .archive: "Archived"
        case .trash: "Moved to Trash"
        case .untrash: "Restored"
        case .unlabel: "Label removed"
        case .release: "Back where it was"
        default: nil
        }
    }

    var systemImage: String {
        switch self {
        case .archive: "archivebox"
        case .unarchive: "tray.and.arrow.up"
        case .trash: "trash"
        case .untrash: "arrow.uturn.backward"
        case .deleteForever: "trash.slash"
        case .markJunk: "xmark.bin"
        case .markNotJunk: "checkmark.shield"
        case .markNotification: "bell"
        case .markPromotion: "tag"
        case .moveToInbox: "tray"
        case .pin: "pin"
        case .unpin: "pin.slash"
        case .snooze: "clock"
        case .unsnooze: "bell"
        case .read: "envelope.open"
        case .unread: "envelope.badge"
        case .star: "star"
        case .unstar: "star.slash"
        case .label: "tag"
        case .unlabel: "tag.slash"
        case .release: "checkmark.shield"
        }
    }

    /// The verb `POST /api/conversations/batch` takes for it, or nil.
    var batch: String? {
        switch self {
        case .archive: "archive"
        case .unarchive: "unarchive"
        case .trash: "delete"
        case .untrash: "untrash"
        case .read: "read"
        case .unread: "unread"
        case .star: "star"
        case .unstar: "unstar"
        case .label: "label"
        case .unlabel: "unlabel"
        default: nil
        }
    }

    /// Whether the row leaves the list it is shown in once this is done.
    var leavesList: Bool {
        switch self {
        case .pin, .unpin, .read, .unread, .star, .unstar, .label: false
        default: true
        }
    }

    /// Only what cannot be undone asks first.
    var confirm: Bool { self == .deleteForever }

    /// The action that takes this one back, or nil when nothing does.
    var undo: RowAction? {
        switch self {
        case .archive: .unarchive
        case .unarchive: .archive
        case .trash: .untrash
        case .untrash: .trash
        case .deleteForever, .moveToInbox, .release: nil
        case .markJunk: .markNotJunk
        case .markNotJunk: .markJunk
        case .markNotification, .markPromotion: .moveToInbox
        case .pin: .unpin
        case .unpin: .pin
        case .snooze: .unsnooze
        case .unsnooze: .snooze
        case .read: .unread
        case .unread: .read
        case .star: .unstar
        case .unstar: .star
        case .label: .unlabel
        case .unlabel: .label
        }
    }
}

/// Which actions a conversation offers where it is shown, in the web's
/// order (`rowActions` in `actions.ts`), so a person finds them in the
/// same place on every surface.
enum RowActions {
    static func forRow(
        _ c: Wire.Conversation, list: MailList, inLabel: Bool, now: Int64
    ) -> [RowAction] {
        if list == .review { return [.release, .trash] }
        if c.isTrashed || list == .trash {
            return [pick(c.unreadCount > 0, .read, .unread), .untrash, .deleteForever]
        }
        var out: [RowAction] = [
            pick(c.unreadCount > 0, .read, .unread),
            pick(c.flagged, .unstar, .star),
            pick(c.pinned, .unpin, .pin),
            pick(c.archived, .unarchive, .archive),
            pick((c.snoozedUntil ?? 0) > now, .unsnooze, .snooze),
        ]
        out += buckets(list)
        out.append(.label)
        if inLabel { out.append(.unlabel) }
        out.append(.trash)
        return out
    }

    /// What a selection of several rows offers: the actions that do not
    /// depend on one row's own state, for the list they are in.
    static func forSelection(list: MailList, inLabel: Bool) -> [RowAction] {
        switch list {
        case .review: return [.release, .trash]
        case .trash: return [.untrash, .deleteForever]
        default:
            var out: [RowAction] = [.unread, .pin, .snooze]
            out += buckets(list)
            out.append(.label)
            if inLabel { out.append(.unlabel) }
            out.append(.trash)
            return out
        }
    }

    private static func buckets(_ list: MailList) -> [RowAction] {
        switch list {
        case .junk: return [.markNotJunk]
        case .np: return [.moveToInbox, .markJunk]
        default: return [.markNotification, .markPromotion, .markJunk]
        }
    }

    /// Of a pair, the one that applies now.
    private static func pick(_ on: Bool, _ ifOn: RowAction, _ ifOff: RowAction) -> RowAction {
        if on { return ifOn }
        return ifOff
    }
}
