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
@Suite struct MailboxApplyTests {
    private func row(
        _ account: String, _ uid: UInt32, date: Int64?, seen: Bool = false,
        folder: String = "INBOX", subject: String = "s"
    ) -> MailboxRow {
        MailboxRow(
            accountId: account, uid: uid, folder: folder, seen: seen,
            sender: "a@x.jp", subject: subject, date: date, messageId: "<\(account)-\(uid)>")
    }

    /// A pass that re-reads a folder from the start — which is what a
    /// renumbering forces — would otherwise double every message.
    @Test func aMessageReadTwiceIsOneRow() {
        let held = [row("a", 1, date: 1), row("a", 2, date: 2)]
        let out = MailboxApply.apply(held: held, fetched: [row("a", 1, date: 1)])
        #expect(out.count == 2)
    }

    /// **The server's flags win.** It knows; this end is holding what
    /// it knew last time, and a mailbox read on a phone and a laptop
    /// disagrees within minutes otherwise.
    @Test func theServersFlagsWin() {
        let held = [row("a", 1, date: 1, seen: false)]
        let out = MailboxApply.apply(held: held, fetched: [row("a", 1, date: 1, seen: true)])
        #expect(out.first?.seen == true)
    }

    @Test func newMessagesAreKept() {
        let out = MailboxApply.apply(
            held: [row("a", 1, date: 1)], fetched: [row("a", 2, date: 2)])
        #expect(out.map(\.uid).sorted() == [1, 2])
    }

    /// Every uid held for a renumbered folder is a number that no
    /// longer means anything: keeping them beside the fresh ones
    /// leaves a list of messages that cannot be opened.
    @Test func aRenumberedFolderIsReplacedNotMerged() {
        let held = [
            row("a", 1, date: 1), row("a", 2, date: 2),
            row("a", 9, date: 9, folder: "Archive"),
            row("b", 1, date: 1),
        ]
        let out = MailboxApply.replacingFolder(
            held: held, accountId: "a", folder: "INBOX", with: [row("a", 1, date: 5)])
        // The other folder and the other account are untouched.
        #expect(out.contains { $0.accountId == "a" && $0.folder == "Archive" })
        #expect(out.contains { $0.accountId == "b" })
        // And INBOX holds only what the pass just read.
        #expect(out.filter { $0.accountId == "a" && $0.folder == "INBOX" }.count == 1)
    }

    /// A row left behind when its account is removed is mail nobody
    /// can open — the credential and the server are both gone.
    @Test func removingAnAccountTakesItsMailWithIt() {
        let rows = [row("a", 1, date: 1), row("b", 2, date: 2)]
        #expect(MailboxApply.withoutAccount(rows, "a").map(\.accountId) == ["b"])
    }
}
