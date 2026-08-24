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
    /// How big the whole message is, when the server said.
    ///
    /// On the row so a reader can be told what opening one would cost
    /// before it costs it — a message with a 25 MB attachment is 25 MB
    /// to fetch, and on mobile data that is a decision rather than a
    /// tap.
    var size: Int64?

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
    /// How old what is on screen is, across every account.
    ///
    /// **The oldest, not the newest**, and never a guess. With three
    /// accounts where two synced a minute ago and one has been failing
    /// since yesterday, "updated just now" is a lie about the third —
    /// and the whole reason to show a time is to tell "no new mail"
    /// apart from "we have not managed to check".
    ///
    /// `nil` when any account has never synced at all, because then
    /// some of the mail has never been fetched and no time describes
    /// the screen.
    static func oldestSync(_ accountIds: [String], _ lastSync: (String) -> Int64?) -> Int64? {
        guard !accountIds.isEmpty else { return nil }
        var oldest = Int64.max
        for id in accountIds {
            guard let at = lastSync(id) else { return nil }
            oldest = min(oldest, at)
        }
        return oldest
    }

    /// Unread per account, for the filter to say which is worth
    /// looking at.
    ///
    /// **Accounts with none are absent from the map, not zero.** A
    /// badge reading `0` is a badge that says nothing while taking up
    /// the space of one that would, and every mail client hides it.
    static func unreadPerAccount(_ rows: [MailboxRow]) -> [String: Int] {
        var out: [String: Int] = [:]
        for row in rows where !row.seen { out[row.accountId, default: 0] += 1 }
        return out
    }

    static func unreadCount(_ rows: [MailboxRow]) -> Int {
        rows.lazy.filter { !$0.seen }.count
    }
}


/// Folding a pass's worth of rows into what is already held.
enum MailboxApply {
    /// How many rows one account may keep.
    ///
    /// **Raised from 2,000 when the rows moved into SQLite**, because
    /// the old number was chosen for a cost that no longer exists.
    /// Every row used to live in one `UserDefaults` blob, held in
    /// memory and rewritten whole on every change, so an unbounded
    /// list made each swipe-to-delete a rewrite of everything. A table
    /// addresses the row that changed, and the limit now bounds disk
    /// instead.
    ///
    /// That mattered for more than tidiness: "load earlier" fetches
    /// **older** mail, and a cap that keeps only the newest 2,000
    /// threw it away in the same pass that fetched it. On a mailbox
    /// with more than 2,000 messages the button did nothing at all,
    /// and did it slowly — which is not a shape any test with a
    /// three-message script can see.
    ///
    /// **5,000 is set by the list, not by the disk.** The screen still
    /// reads every row into memory and sorts them there, so the number
    /// that binds is six accounts × this × roughly 300 bytes a row —
    /// about 9 MB, which a phone can hold. On disk it is nothing.
    ///
    /// Raising it further is gated on the list reading a window
    /// (`ORDER BY … LIMIT`) instead of everything, which is what a
    /// table makes possible and a blob did not. Until then a larger
    /// number would trade a bounded store for an unbounded read, which
    /// is the same defect facing the other way.
    static let perAccount = 5_000

    /// Keep at most `limit` rows per account.
    ///
    /// **No production caller since the rows moved into SQLite** — the
    /// table does its own capping, in SQL, because deciding what to
    /// drop by loading everything is the cost that move was about.
    /// This stays as the readable statement of the rule, and a test
    /// holds the SQL to it. Delete both together or neither.
    ///
    /// **Per account, not overall.** One noisy mailbox would otherwise
    /// evict a quiet one entirely, and the quiet one is where the mail
    /// a person is waiting for tends to be.
    static func capped(_ rows: [MailboxRow], limit: Int = perAccount) -> [MailboxRow] {
        var byAccount: [String: [MailboxRow]] = [:]
        for row in rows { byAccount[row.accountId, default: []].append(row) }
        guard byAccount.values.contains(where: { $0.count > limit }) else { return rows }
        var keep: Set<String> = []
        for (_, owned) in byAccount {
            for row in MailboxMerge.newestFirst(owned).prefix(limit) { keep.insert(row.id) }
        }
        // Filtered rather than rebuilt from the groups, so the order
        // rows were held in survives — the list sorts them itself, and
        // reshuffling storage on every pass makes diffs unreadable.
        return rows.filter { keep.contains($0.id) }
    }
}
