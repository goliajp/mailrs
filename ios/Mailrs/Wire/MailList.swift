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
            URLQueryItem(name: "archived", value: archived ? "true" : "false")
        ]
        if let folder { items.append(URLQueryItem(name: "folder", value: folder)) }
        if let unread { items.append(URLQueryItem(name: "unread", value: unread ? "true" : "false")) }
        if let starred { items.append(URLQueryItem(name: "starred", value: starred ? "true" : "false")) }
        return items
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
    case junk
    case archived
    /// Not a thread list: two endpoints joined by `SendJoin`. "Send",
    /// not "Sent" — the rows include mail that failed and mail still
    /// going out, and a heading claiming they were sent would be wrong
    /// about exactly the rows worth looking at.
    case send

    var id: String { rawValue }

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
        case .send: "Send"
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
        // Unused: Send never queries /api/conversations. The axes exist
        // so the type stays total; `Session` branches on the case.
        case .send: MailListAxes()
        }
    }
}
