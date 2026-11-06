import Foundation
import SwiftUI

/// The axes a list asks the server for.
///
/// `ListQuery` in `conversations.rs` takes `folder`, `unread`, `starred`
/// and `archived`; a list is a fixed combination of them and nothing
/// else. Search takes the same four, which is why they live in one type
/// rather than as separate arguments — the list and the search inside it
/// must scope to the same thing or searching Junk quietly returns Inbox.
struct MailListAxes: Equatable, Sendable {
    var folder: String?
    var unread: Bool?
    var starred: Bool?
    var archived: Bool = false
    /// The Trash; every other list leaves it out.
    var trashed: Bool = false
    var pinned: Bool = false
    var snoozed: Bool = false
    /// One label's conversations, by label id.
    var label: String?
    /// Which connected mailboxes to narrow to.
    ///
    /// `nil` is every account, and it is not the same as an empty
    /// array: somebody who has unticked every box is asking for
    /// nothing, and answering with everything would answer a question
    /// they did not ask. This server's own mail is the empty string,
    /// so it can be switched off like any other.
    var accounts: [String]?
    /// One section of the sectioned inbox (`reply`, `awaiting`, `new`);
    /// nil is the whole inbox.
    var section: String?
    /// Review: listed from `/api/quarantine`, so only its search reads this.
    var held: Bool = false
}

extension MailListAxes {
    /// The query items both `/api/conversations` and its `/search` take.
    ///
    /// Absent rather than false for the optional flags: `unread=false`
    /// asks for threads that are *read*, which is a different list from
    /// "not filtered by unread". `archived` is the exception — the
    /// handler declares it `#[serde(default)]` and always means a
    /// boolean, so it is always sent.
    var queryItems: [URLQueryItem] {
        var items: [URLQueryItem] = [
            URLQueryItem(name: "archived", value: MailListAxes.flag(archived))
        ]
        if let folder { items.append(URLQueryItem(name: "folder", value: folder)) }
        if let unread {
            items.append(URLQueryItem(name: "unread", value: MailListAxes.flag(unread)))
        }
        if let starred {
            items.append(URLQueryItem(name: "starred", value: MailListAxes.flag(starred)))
        }
        // present only when asked for: each is a list of its own, not a
        // filter that is false everywhere else
        if trashed { items.append(URLQueryItem(name: "trashed", value: "true")) }
        if pinned { items.append(URLQueryItem(name: "pinned", value: "true")) }
        if snoozed { items.append(URLQueryItem(name: "snoozed", value: "true")) }
        if let label { items.append(URLQueryItem(name: "label", value: label)) }
        if let section { items.append(URLQueryItem(name: "section", value: section)) }
        // Sent whenever the filter is narrowed — including to nothing,
        // which is the parameter present and empty. The server reads an
        // absent one as every account, so the two cannot be collapsed.
        if let accounts {
            items.append(URLQueryItem(name: "accounts", value: accounts.joined(separator: ",")))
        }
        // a search typed into Review names it, or it searches every other list
        if held { items.append(URLQueryItem(name: "quarantined", value: "true")) }
        return items
    }

    /// The server reads these as the strings, not as JSON booleans.
    static func flag(_ value: Bool) -> String {
        if value { return "true" }
        return "false"
    }
}

/// The lists this app shows, and what each one is.
///
/// The same set the web client declares in `lib/mail-lists.ts`, minus
/// Send and Draft — those read different endpoints entirely and are not
/// a folder. Keeping the axes here rather than at the call sites is what
/// stops "which threads is Starred" from being answered differently by
/// the list, the search and the unread count.
enum MailList: String, CaseIterable, Identifiable, Sendable {
    case inbox
    case np
    case unread
    case starred
    case pinned
    case snoozed
    /// Everything but Junk and the Trash, archived included.
    case all
    case archived
    case junk
    /// Held as suspected fraud: `GET /api/quarantine`, not a folder.
    case review
    case trash
    /// One of the reader's labels; which one is `Session.activeLabel`.
    /// Not drawn with the fixed lists: each label is a row of its own.
    case label
    /// Not a thread list: two endpoints joined by `SendJoin`. Titled
    /// "Sent" like the folder in every other client; each row's status
    /// badge says when a message failed or is still going out. The
    /// title must not share the compose button's "Send" key.
    case send

    var id: String { rawValue }

    /// The lists drawn in the sidebar, in the web's order. Labels follow
    /// them, one row each.
    static let shown: [MailList] = allCases.filter { $0 != .label }

    /// The catalog key. Kept as a `String` alongside the view-facing
    /// `title` so a test can ask whether the catalog carries it —
    /// `LocalizedStringKey` is opaque and cannot be asked anything.
    var titleKey: String {
        switch self {
        case .inbox: "Inbox"
        case .np: "N & P"
        case .unread: "Unread"
        case .starred: "Starred"
        case .junk: "Junk"
        case .archived: "Archived"
        case .pinned: "Pinned"
        case .snoozed: "Snoozed"
        case .all: "All mail"
        case .review: "Review"
        case .trash: "Trash"
        case .label: "Label"
        case .send: "Sent"
        }
    }

    /// `LocalizedStringKey`, not `String`: `Text(aString)` is the
    /// verbatim initialiser and never consults a localization table,
    /// so a list title returned as `String` would stay English in
    /// every language.
    var title: LocalizedStringKey { LocalizedStringKey(titleKey) }

    var systemImage: String {
        switch self {
        case .inbox: "tray"
        case .np: "megaphone"
        case .unread: "envelope.badge"
        case .starred: "star"
        case .junk: "xmark.bin"
        case .archived: "archivebox"
        case .pinned: "pin"
        case .snoozed: "clock"
        case .all: "tray.2"
        case .review: "exclamationmark.shield"
        case .trash: "trash"
        case .label: "tag"
        case .send: "paperplane"
        }
    }

    /// What the list shows when it is empty. Written per list because
    /// "All caught up" is wrong for Junk and alarming for Archived.
    var emptyMessageKey: String {
        switch self {
        case .inbox: "All caught up"
        case .np: "Nothing here"
        case .unread: "All caught up"
        case .starred: "Nothing starred"
        case .junk: "No junk mail"
        case .archived: "No archived conversations"
        case .pinned: "Nothing pinned"
        case .snoozed: "Nothing snoozed"
        case .all: "No mail"
        case .review: "Nothing held. No one is currently trying to defraud you."
        case .trash: "Trash is empty"
        case .label: "No conversations with this label"
        case .send: "Nothing sent yet"
        }
    }

    var emptyMessage: LocalizedStringKey { LocalizedStringKey(emptyMessageKey) }

    var axes: MailListAxes {
        switch self {
        case .inbox: MailListAxes(folder: "Inbox")
        // The server merges the Notifications and Promotions buckets for
        // this one; `NP` is the name its folder parser knows.
        case .np: MailListAxes(folder: "NP")
        // `NonJunk`, not nil: unread and starred are attributes of a
        // thread rather than places one lives, and scoping them to
        // everything would drag Junk back out of the one surface it is
        // allowed to have.
        case .unread: MailListAxes(folder: "NonJunk", unread: true)
        case .starred: MailListAxes(folder: "NonJunk", starred: true)
        case .junk: MailListAxes(folder: "Junk")
        // No folder. Archived is cross-folder — the server drops the
        // folder when this is set, because "archived within Inbox" is not
        // what the tab means.
        case .archived: MailListAxes(archived: true)
        // across folders, as Starred is
        case .pinned: MailListAxes(pinned: true)
        case .snoozed: MailListAxes(snoozed: true)
        case .all: MailListAxes(folder: "all")
        case .trash: MailListAxes(trashed: true)
        // The list reads `/api/quarantine`, see `Session`; only its search
        // reads these axes.
        case .review: MailListAxes(held: true)
        // Unused: the label's id is `Session.activeLabel`'s.
        case .label: MailListAxes()
        // Unused: Send never queries /api/conversations. The axes exist
        // so the type stays total; `Session` branches on the case.
        case .send: MailListAxes()
        }
    }
}
