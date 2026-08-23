import Testing

@testable import Mailrs

/// Unread per account, for the filter to say which is worth looking
/// at.
@Suite struct UnreadPerAccountTests {
    private func row(_ account: String, _ seen: Bool, _ uid: UInt32) -> MailboxRow {
        MailboxRow(
            accountId: account, uid: uid, folder: "INBOX", seen: seen,
            sender: "s", subject: "x", date: nil, messageId: "m\(uid)")
    }

    @Test func eachAccountIsCountedApart() {
        let rows = [
            row("a", false, 1), row("a", false, 2), row("a", true, 3), row("b", false, 4),
        ]
        #expect(MailboxMerge.unreadPerAccount(rows) == ["a": 2, "b": 1])
    }

    /// **Accounts with none are absent, not zero.** A badge reading
    /// `0` says nothing while taking the space of one that would, and
    /// every mail client hides it.
    @Test func anAccountWithNothingUnreadIsAbsent() {
        let counts = MailboxMerge.unreadPerAccount([row("a", true, 1), row("b", false, 2)])
        #expect(counts["a"] == nil, "an account with nothing unread got a badge")
        #expect(counts["b"] == 1)
    }

    /// Nothing at all is an empty map rather than a crash.
    @Test func noRowsIsNoCounts() {
        #expect(MailboxMerge.unreadPerAccount([]).isEmpty)
    }
}
