import Foundation

/// One line in the list, from whichever mailbox it came.
struct MailboxRow: Equatable, Codable, Identifiable {
    /// Which account this arrived at.
    let accountId: String
    let uid: UInt32
    let folder: String
    var seen: Bool
    var sender: String
    var subject: String
    /// Seconds since the epoch, or nil when the header was unreadable.
    var date: Int64?
    /// The `Message-ID`, which is what survives a renumbering.
    var messageId: String

    /// Unique across accounts.
    ///
    /// A uid is unique **within one folder of one account** and
    /// nowhere else: two accounts both have a message 1, and a list
    /// keyed on uid alone shows one of them twice and the other never.
    var id: String { "\(accountId)/\(folder)/\(uid)" }

    /// What the row shows when the sender is a bare address or is
    /// missing altogether — a blank line where a name goes reads as a
    /// rendering fault rather than as an absent header.
    var displaySender: String {
        sender.trimmingCharacters(in: .whitespaces).isEmpty
            ? "(no sender)" : sender
    }

    /// What the row shows when nobody wrote a subject.
    var displaySubject: String {
        subject.trimmingCharacters(in: .whitespaces).isEmpty
            ? "(no subject)" : subject
    }
}

/// Putting several mailboxes into one list.
enum MailboxMerge {
    /// Newest first, and stable.
    ///
    /// Two messages can carry the same `Date:` — a mailing list fans
    /// one message out to a hundred people in the same second — so the
    /// sort ends on something that cannot tie. Without that the order
    /// changes between two calls with the same input, and a list that
    /// reorders itself while somebody reads it is worse than a wrong
    /// order.
    ///
    /// A row with no readable date sorts **last**, not first: it is
    /// the one thing this client knows nothing about, and putting it
    /// at the top is the position that says "newest".
    static func newestFirst(_ rows: [MailboxRow]) -> [MailboxRow] {
        rows.sorted { a, b in
            switch (a.date, b.date) {
            case let (x?, y?) where x != y: return x > y
            case (nil, _?): return false
            case (_?, nil): return true
            default: return a.id < b.id
            }
        }
    }

    /// Only these accounts, or all of them.
    ///
    /// `nil` is no filter at all — not "every id", which selects the
    /// same rows and says it less clearly. An **empty** set is a
    /// filter nothing satisfies, and that distinction is the point:
    /// somebody who unticked every box gets an empty list rather than
    /// the unfiltered one.
    static func onlyAccounts(_ rows: [MailboxRow], _ ids: Set<String>?) -> [MailboxRow] {
        guard let ids else { return rows }
        return rows.filter { ids.contains($0.accountId) }
    }

    /// How many of these are unread.
    static func unreadCount(_ rows: [MailboxRow]) -> Int {
        rows.lazy.filter { !$0.seen }.count
    }
}


/// Folding a pass's worth of rows into what is already held.
enum MailboxApply {
    /// The held rows, updated by what a pass just read.
    ///
    /// Matched on `id`, so a message read again is the **same row
    /// updated** rather than a second copy. A pass that re-reads a
    /// folder from the start — which is what a renumbering forces —
    /// would otherwise double every message in the list.
    ///
    /// The **server's** flags win. It knows; this end is holding what
    /// it knew last time, and a mailbox read on a phone and a laptop
    /// disagrees within minutes otherwise.
    static func apply(held: [MailboxRow], fetched: [MailboxRow]) -> [MailboxRow] {
        var byId: [String: MailboxRow] = [:]
        var order: [String] = []
        for row in held {
            if byId[row.id] == nil { order.append(row.id) }
            byId[row.id] = row
        }
        for row in fetched {
            if byId[row.id] == nil { order.append(row.id) }
            byId[row.id] = row
        }
        return order.compactMap { byId[$0] }
    }

    /// The rows of one folder replaced wholesale.
    ///
    /// For a renumbering: every uid held for that folder is a number
    /// that no longer means anything, so keeping them beside the fresh
    /// ones leaves a list of messages that cannot be opened.
    static func replacingFolder(
        held: [MailboxRow], accountId: String, folder: String, with fetched: [MailboxRow]
    ) -> [MailboxRow] {
        held.filter { !($0.accountId == accountId && $0.folder == folder) } + fetched
    }

    /// Everything belonging to an account, gone.
    ///
    /// A row left behind when its account is removed is mail nobody
    /// can open — the credential and the server it came from are both
    /// gone.
    static func withoutAccount(_ rows: [MailboxRow], _ accountId: String) -> [MailboxRow] {
        rows.filter { $0.accountId != accountId }
    }
}
