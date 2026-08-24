import Testing

@testable import Mailrs

/// Putting several mailboxes into one list.
@Suite struct MailboxMergeTests {
    private func row(
        _ account: String, _ uid: UInt32, date: Int64?, seen: Bool = false,
        folder: String = "INBOX", subject: String = "s"
    ) -> MailboxRow {
        MailboxRow(
            accountId: account, uid: uid, folder: folder, seen: seen,
            sender: "a@x.jp", subject: subject, date: date, messageId: "<\(account)-\(uid)>")
    }

    /// A uid is unique within one folder of one account and nowhere
    /// else: two accounts both have a message 1, and a list keyed on
    /// uid alone shows one of them twice and the other never.
    @Test func twoAccountsCanBothHaveMessageOne() {
        let a = row("acc_1", 1, date: 100)
        let b = row("acc_2", 1, date: 100)
        #expect(a.id != b.id)
        // And the same message in two folders of one account, which
        // Gmail does with labels.
        #expect(row("acc_1", 1, date: 1, folder: "INBOX").id
            != row("acc_1", 1, date: 1, folder: "Archive").id)
    }

    @Test func theNewestIsFirst() {
        let out = MailboxMerge.newestFirst([
            row("a", 1, date: 100), row("b", 2, date: 300), row("c", 3, date: 200),
        ])
        #expect(out.map(\.uid) == [2, 3, 1])
    }

    /// A mailing list fans one message out to a hundred people in the
    /// same second. Without a tie-break the order changes between two
    /// calls with the same input, and a list that reorders itself
    /// while somebody reads it is worse than a wrong order.
    @Test func aTieIsBrokenBySomethingStable() {
        let rows = [row("b", 2, date: 100), row("a", 1, date: 100), row("c", 3, date: 100)]
        let first = MailboxMerge.newestFirst(rows)
        let again = MailboxMerge.newestFirst(rows.reversed())
        #expect(first.map(\.id) == again.map(\.id), "the order changed with the input order")
    }

    /// The one thing this client knows nothing about must not take the
    /// position that says "newest".
    @Test func aRowWithNoDateSortsLast() {
        let out = MailboxMerge.newestFirst([
            row("a", 1, date: nil), row("b", 2, date: 100),
        ])
        #expect(out.map(\.uid) == [2, 1])
    }

    /// `nil` is no filter; **empty** is a filter nothing satisfies.
    /// Somebody who unticked every box gets an empty list rather than
    /// the unfiltered one.
    @Test func noFilterAndAnEmptyFilterAreDifferentQuestions() {
        let rows = [row("a", 1, date: 1), row("b", 2, date: 2)]
        #expect(MailboxMerge.onlyAccounts(rows, nil).count == 2)
        #expect(MailboxMerge.onlyAccounts(rows, []).isEmpty)
        #expect(MailboxMerge.onlyAccounts(rows, ["a"]).map(\.uid) == [1])
    }

    @Test func theUnreadCountCountsUnread() {
        let rows = [
            row("a", 1, date: 1, seen: false), row("a", 2, date: 2, seen: true),
            row("b", 3, date: 3, seen: false),
        ]
        #expect(MailboxMerge.unreadCount(rows) == 2)
    }

    /// Plenty of real mail has no subject, and an empty line in a list
    /// reads as a rendering fault.
    @Test func aMessageWithNoSubjectStillHasALine() {
        #expect(row("a", 1, date: 1, subject: "").displaySubject == "(no subject)")
        #expect(row("a", 1, date: 1, subject: "   ").displaySubject == "(no subject)")
        #expect(row("a", 1, date: 1, subject: "real").displaySubject == "real")
    }
}

/// Folding a pass's worth of rows into what is already held.

// MailboxApplyTests stood here. Its five assertions — a message read
// twice is one row, the server's flags win, new messages are kept, a
// renumbered folder is replaced rather than merged, and removing an
// account takes its mail — moved to MailboxDatabaseTests when the rows
// moved into SQLite. They are properties of the store, and the store is
// now the table; keeping them against a list that no production code
// builds any more would have been a suite that stays green while the
// thing it names breaks. MailboxApply.capped survives, with its own
// tests in MailboxCapTests, as the rule the SQL cap is checked against.
