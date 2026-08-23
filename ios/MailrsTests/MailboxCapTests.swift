import Testing

@testable import Mailrs

/// How many rows one account may keep.
@Suite struct MailboxCapTests {
    private func row(_ account: String, _ uid: UInt32, _ date: Int64?) -> MailboxRow {
        MailboxRow(
            accountId: account, uid: uid, folder: "INBOX", seen: false,
            sender: "s", subject: "x", date: date, messageId: "m\(uid)")
    }

    /// Under the limit, nothing is touched at all.
    @Test func aShortListIsReturnedAsItWas() {
        let rows = (1...10).map { row("a", UInt32($0), Int64($0)) }
        #expect(MailboxApply.capped(rows, limit: 100) == rows)
    }

    /// What falls off is what somebody would have scrolled furthest to
    /// see — the same order the list itself uses.
    @Test func theOldestGoFirst() {
        let rows = (1...10).map { row("a", UInt32($0), Int64($0)) }
        #expect(MailboxApply.capped(rows, limit: 3).map(\.uid) == [8, 9, 10])
    }

    /// **Per account, not overall.** One noisy mailbox would otherwise
    /// evict a quiet one entirely, and the quiet one is where the mail
    /// somebody is waiting for tends to be.
    @Test func aNoisyAccountCannotEvictAQuietOne() {
        let noisy = (1...100).map { row("noisy", UInt32($0), Int64(1000 + $0)) }
        let quiet = [row("quiet", 1, 1)]
        let kept = MailboxApply.capped(noisy + quiet, limit: 10)
        #expect(kept.filter { $0.accountId == "noisy" }.count == 10)
        #expect(kept.filter { $0.accountId == "quiet" }.count == 1)
    }

    /// The stored order survives: the list sorts rows itself, and
    /// reshuffling storage on every pass makes a diff unreadable.
    @Test func theHeldOrderIsNotRearranged() {
        let rows = [row("a", 3, 3), row("a", 1, 1), row("a", 2, 2)]
        #expect(MailboxApply.capped(rows, limit: 2).map(\.uid) == [3, 2])
    }

    /// A row with no date is still a row, and it is not preferred
    /// away.
    @Test func rowsWithoutADateAreKeptWhenThereIsRoom() {
        #expect(MailboxApply.capped([row("a", 1, nil), row("a", 2, 5)], limit: 5).count == 2)
    }
}
