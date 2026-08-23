import Testing

@testable import Mailrs

/// What a pass learned about messages this device already had.
@Suite struct MailboxRefreshTests {
    private func row(
        _ uid: UInt32, _ seen: Bool, folder: String = "INBOX", account: String = "a"
    ) -> MailboxRow {
        MailboxRow(
            accountId: account, uid: uid, folder: folder, seen: seen,
            sender: "s", subject: "x", date: nil, messageId: "m\(uid)")
    }

    /// A message read on a laptop stops being bold here.
    @Test func aFlagChangedElsewhereIsPickedUp() {
        let after = MailboxRefresh.apply(
            held: [row(1, false), row(2, false)], accountId: "a", folder: "INBOX",
            asked: [1, 2], answer: [1: true, 2: false])
        #expect(after[0].seen)
        #expect(after[1].seen == false)
    }

    /// **A uid asked about that did not come back is gone.** Without
    /// this, a message deleted on another device stays in the list
    /// forever.
    @Test func aMessageDeletedElsewhereIsRemoved() {
        let after = MailboxRefresh.apply(
            held: [row(1, false), row(2, false), row(3, false)], accountId: "a",
            folder: "INBOX", asked: [1, 2, 3], answer: [1: false, 3: false])
        #expect(after.map(\.uid) == [1, 3])
    }

    /// **Only rows that were asked about may be removed.** A partial
    /// or interrupted fetch would otherwise empty the list — the
    /// answer is silent about everything the question did not name.
    @Test func aRowThatWasNotAskedAboutSurvivesAnEmptyAnswer() {
        let after = MailboxRefresh.apply(
            held: [row(1, false), row(2, false)], accountId: "a", folder: "INBOX",
            asked: [1], answer: [:])
        #expect(after.map(\.uid) == [2])
    }

    /// And an answer about one folder says nothing about another.
    @Test func anotherFolderIsUntouched() {
        let after = MailboxRefresh.apply(
            held: [row(1, false), row(1, false, folder: "Sent")], accountId: "a",
            folder: "INBOX", asked: [1], answer: [:])
        #expect(after.count == 1)
        #expect(after.first?.folder == "Sent")
    }

    /// Nor about another account, whose uids look exactly the same.
    @Test func anotherAccountIsUntouched() {
        let after = MailboxRefresh.apply(
            held: [row(1, false), row(1, false, account: "b")], accountId: "a",
            folder: "INBOX", asked: [1], answer: [:])
        #expect(after.count == 1)
        #expect(after.first?.accountId == "b")
    }

    /// Nothing asked, nothing changed.
    @Test func askingAboutNothingChangesNothing() {
        let held = [row(1, true), row(2, false)]
        #expect(
            MailboxRefresh.apply(
                held: held, accountId: "a", folder: "INBOX", asked: [], answer: [:]) == held)
        #expect(
            MailboxRefresh.apply(
                held: [], accountId: "a", folder: "INBOX", asked: [1], answer: [:]).isEmpty)
    }
}
