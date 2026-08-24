import Foundation
import Testing

@testable import Mailrs

/// The store, against a real SQLite file in a temporary directory.
///
/// Five of these are the assertions that used to be made against a
/// list in MailboxMergeTests — they are properties of *where the rows
/// are kept*, and where they are kept changed. The rest are things a
/// blob could not have got wrong because it rewrote everything every
/// time, and a table can.
struct MailboxDatabaseTests {
    private func open() throws -> (MailboxDatabase, URL) {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("mailrs-test-\(UUID().uuidString).sqlite")
        return (try MailboxDatabase(path: url.path), url)
    }

    private func row(
        _ account: String, _ uid: UInt32, date: Int64? = nil,
        folder: String = "INBOX", seen: Bool = false
    ) -> MailboxRow {
        MailboxRow(
            accountId: account, uid: uid, folder: folder, seen: seen,
            sender: "a@x.jp", subject: "s", date: date ?? Int64(uid),
            messageId: "<\(account)-\(uid)>")
    }

    @Test func aMessageReadTwiceIsOneRow() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1), row("a", 2)])
        try db.upsert([row("a", 1)])
        #expect(try db.all().count == 2)
    }

    @Test func theServersFlagsWin() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1, seen: false)])
        try db.upsert([row("a", 1, seen: true)])
        #expect(try db.all().first?.seen == true)
    }

    @Test func newMessagesAreKept() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1)])
        try db.upsert([row("a", 2)])
        #expect(try db.all().map(\.uid).sorted() == [1, 2])
    }

    // Every uid held for a renumbered folder is a number that no
    // longer means anything — and nothing else may be caught by that.
    @Test func aRenumberedFolderIsDroppedAndNothingElseIs() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([
            row("a", 1), row("a", 2), row("a", 9, folder: "Archive"), row("b", 1),
        ])
        try db.deleteFolder(account: "a", folder: "INBOX")
        let left = try db.all()
        #expect(left.contains { $0.accountId == "a" && $0.folder == "Archive" })
        #expect(left.contains { $0.accountId == "b" })
        #expect(!left.contains { $0.accountId == "a" && $0.folder == "INBOX" })
    }

    @Test func removingAnAccountTakesItsMailWithIt() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1), row("b", 2)])
        try db.deleteAccount("a")
        #expect(try db.all().map(\.accountId) == ["b"])
    }

    // The same uid in two accounts is two messages. A table keyed on
    // uid alone would have shown one of them twice and the other never
    // — which is the reason MailboxRow.id is spelled the way it is.
    @Test func theSameUidInTwoAccountsIsTwoRows() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1), row("b", 1), row("a", 1, folder: "Archive")])
        #expect(try db.all().count == 3)
    }

    @Test func deletingAddressesOneRow() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1), row("a", 2), row("b", 1)])
        try db.delete(account: "a", folder: "INBOX", uid: 1)
        #expect(try Set(db.all().map(\.id)) == ["a/INBOX/2", "b/INBOX/1"])
    }

    @Test func aFlagChangeTouchesNothingElse() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1), row("a", 2), row("b", 1)])
        try db.setSeen(account: "a", folder: "INBOX", uid: 1, seen: true)
        #expect(try Set(db.all().filter(\.seen).map(\.id)) == ["a/INBOX/1"])
    }

    // A row's date may be absent — a header a server would not give up
    // — and null is not zero: it sorts last rather than to 1970.
    @Test func aRowWithNoDateSurvivesARoundTrip() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        var without = row("a", 1)
        without.date = nil
        without.size = nil
        try db.upsert([without])
        let back = try db.all().first
        #expect(back?.date == nil)
        #expect(back?.size == nil)
    }

    @Test func aSizeSurvivesARoundTrip() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        var big = row("a", 1)
        big.size = 26_214_400
        try db.upsert([big])
        #expect(try db.all().first?.size == 26_214_400)
    }

    // Non-ASCII in a subject is the ordinary case for this app, and a
    // C string interface is where it stops being ordinary.
    @Test func aJapaneseSubjectSurvivesARoundTrip() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        var row = row("a", 1)
        row.subject = "領収書のご送付について 🧾"
        row.sender = "山田 太郎"
        try db.upsert([row])
        let back = try db.all().first
        #expect(back?.subject == "領収書のご送付について 🧾")
        #expect(back?.sender == "山田 太郎")
    }

    // A quote in a value reaching SQL as syntax rather than as text is
    // the oldest mistake there is, and a subject line is attacker-set.
    @Test func aQuoteInASubjectIsText() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        var row = row("a", 1)
        row.subject = "'; DROP TABLE rows; --"
        try db.upsert([row])
        #expect(try db.all().first?.subject == "'; DROP TABLE rows; --")
    }

    @Test func theCapIsPerAccount() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert((1...10).map { row("a", UInt32($0)) }
            + (1...3).map { row("b", UInt32($0)) })
        try db.cap(account: "a", limit: 4)
        #expect(try db.all().filter { $0.accountId == "a" }.count == 4)
        #expect(try db.all().filter { $0.accountId == "b" }.count == 3)
    }

    /// The SQL cap and `MailboxApply.capped` drop the same rows.
    ///
    /// Not "the newest four survive" — that is a claim about an
    /// ordering I would be writing twice, and the second copy is the
    /// one that drifts. This asks the two implementations the same
    /// question and requires the same answer, with a tie on date and
    /// an absent date both in the sample because those are where an
    /// ordering differs without anybody noticing.
    @Test func theSqlCapAgreesWithTheRuleItWasWrittenFrom() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        var undated = row("a", 4)
        undated.date = nil
        var alsoUndated = row("a", 7)
        alsoUndated.date = nil
        let rows = [
            row("a", 1, date: 100), row("a", 2, date: 100), row("a", 3, date: 300),
            row("a", 10, date: 300), undated, row("a", 5, date: 500),
            row("a", 6, date: 50, folder: "Archive"), alsoUndated,
        ]
        for limit in 0...rows.count {
            try db.replaceAll(rows)
            try db.cap(account: "a", limit: limit)
            #expect(
                Set(MailboxApply.capped(rows, limit: limit).map(\.id))
                    == Set(try db.all().map(\.id)),
                "limit=\(limit)")
        }
    }

    @Test func replacingKeepsOnlyWhatItWasGiven() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1), row("b", 2)])
        try db.replaceAll([row("c", 3)])
        #expect(try db.all().map(\.id) == ["c/INBOX/3"])
    }

    @Test func anEmptyWriteIsNotAWipe() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1)])
        try db.upsert([])
        try db.delete(account: "a", folder: "INBOX", uids: [])
        try db.setSeen(account: "a", folder: "INBOX", flags: [:])
        #expect(try db.all().count == 1)
        #expect(try db.all().first?.seen == false)
    }

    // The rows outlive the process; a cache that empties on every
    // launch is not a cache.
    @Test func rowsSurviveReopening() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1)])
        let again = try MailboxDatabase(path: url.path)
        #expect(try again.all().map(\.id) == ["a/INBOX/1"])
    }

    // MARK: - the windowed read
    //
    // The list used to load everything and sort it in memory. These
    // hold the SQL to the rules that read did, rather than to a
    // remembered ordering — two spellings of "newest first" is two
    // orders that agree until they do not.

    private func sample() -> [MailboxRow] {
        var undated = row("b", 4)
        undated.date = nil
        return [
            row("a", 1, date: 100), row("a", 2, date: 300),
            row("b", 3, date: 200), undated,
            row("a", 5, date: 300, folder: "Archive"),
        ]
    }

    @Test func theWindowIsTheSameOrderTheListUses() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert(sample())
        #expect(
            try db.newest(limit: sample().count).map(\.id)
                == MailboxMerge.newestFirst(sample()).map(\.id))
    }

    @Test func theWindowTakesTheNewestNotJustAny() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert(sample())
        #expect(
            try db.newest(limit: 2).map(\.id)
                == Array(MailboxMerge.newestFirst(sample()).map(\.id).prefix(2)))
    }

    // Nil is no filter; an **empty set** is a filter nothing
    // satisfies. Somebody who unticked every box gets an empty list,
    // not the unfiltered one.
    @Test func anEmptyFilterIsNotNoFilter() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert(sample())
        #expect(try db.newest(limit: 50, accounts: nil).count == 5)
        #expect(try db.newest(limit: 50, accounts: []).isEmpty)
        #expect(try db.newest(limit: 50, accounts: ["a"]).count == 3)
    }

    @Test func theSqlSearchAgreesWithTheRuleItWasWrittenFrom() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        var one = row("a", 1, date: 300)
        one.sender = "Ada"
        one.subject = "Lunch"
        var two = row("a", 2, date: 200)
        two.sender = "Bob"
        two.subject = "Lunch tomorrow"
        var three = row("b", 3, date: 100)
        three.sender = "Ada"
        three.subject = "Dinner"
        let rows = [one, two, three]
        try db.upsert(rows)
        for query in ["lunch", "ada", "ada lunch", "ada dinner", "", "zzz"] {
            #expect(
                try db.search(words: MailboxSearch.words(of: query), limit: 50).map(\.id)
                    == MailboxSearch.matches(MailboxMerge.newestFirst(rows), query).map(\.id),
                "\(query)")
        }
    }

    // **Where the two would have parted.** SQLite's `lower` folds
    // ASCII and nothing else, so a subject with an accent matched in
    // memory and not in SQL — a divergence in exactly the alphabets
    // nobody writes a test with. The folded text is stored instead, by
    // the same function the in-memory search uses.
    @Test func anAccentedSubjectIsFoundTheSameWayBothWays() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        var one = row("a", 1, date: 300)
        one.sender = "Ämile"
        one.subject = "RÉUNION"
        var two = row("a", 2, date: 200)
        two.sender = "山田 太郎"
        two.subject = "領収書"
        let rows = [one, two]
        try db.upsert(rows)
        for query in ["réunion", "ämile", "領収書", "RÉUNION"] {
            #expect(
                try db.search(words: MailboxSearch.words(of: query), limit: 50).map(\.id)
                    == MailboxSearch.matches(MailboxMerge.newestFirst(rows), query).map(\.id),
                "\(query)")
        }
    }

    // A row edited must not keep the text it used to be searchable by.
    @Test func theFoldedTextFollowsAnUpdate() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        var before = row("a", 1)
        before.subject = "Lunch"
        try db.upsert([before])
        var after = row("a", 1)
        after.subject = "Dinner"
        try db.upsert([after])
        #expect(try db.search(words: ["lunch"], limit: 50).isEmpty)
        #expect(try db.search(words: ["dinner"], limit: 50).count == 1)
    }

    @Test func unreadIsCountedPerAccountOverEverything() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([
            row("a", 1, seen: false), row("a", 2, seen: true),
            row("a", 3, seen: false), row("b", 4, seen: false),
        ])
        #expect(try db.unreadPerAccount() == ["a": 2, "b": 1])
    }

    // An account with nothing unread is **absent**, not zero — the
    // chip reads the map and a 0 would draw an empty badge.
    @Test func anAccountWithNothingUnreadIsAbsent() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1, seen: true)])
        #expect(try db.unreadPerAccount().isEmpty)
    }

    @Test func foldersAreTheOnesThisDeviceHoldsSomethingOf() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([
            row("a", 1), row("a", 2, folder: "Archive"),
            row("a", 3, folder: "Archive"), row("b", 4, folder: "Sent"),
        ])
        #expect(try Set(db.folders(account: "a")) == ["INBOX", "Archive"])
        #expect(try db.folders(account: "b") == ["Sent"])
    }

    @Test func countIsPerAccount() throws {
        let (db, url) = try open()
        defer { try? FileManager.default.removeItem(at: url) }
        try db.upsert([row("a", 1), row("a", 2), row("b", 3)])
        #expect(try db.count(account: "a") == 2)
        #expect(try db.count(account: "nobody") == 0)
    }
}
