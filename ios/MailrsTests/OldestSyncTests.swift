import Testing

@testable import Mailrs

/// How old what is on screen is, across every account.
@Suite struct OldestSyncTests {
    /// **The oldest, not the newest.** With two accounts synced a
    /// minute ago and one failing since yesterday, "updated just now"
    /// is a lie about the third — and telling "no new mail" apart from
    /// "we have not managed to check" is the whole reason the line
    /// exists.
    @Test func theOldestAccountDecides() {
        let times: [String: Int64] = ["a": 1000, "b": 5000, "c": 3000]
        #expect(MailboxMerge.oldestSync(["a", "b", "c"], { times[$0] }) == 1000)
    }

    /// An account that has never synced makes the whole line unknown:
    /// some of the mail has never been fetched, and no time describes
    /// the screen.
    @Test func anAccountThatNeverSyncedMakesItUnknown() {
        let times: [String: Int64] = ["a": 1000]
        #expect(MailboxMerge.oldestSync(["a", "b"], { times[$0] }) == nil)
    }

    /// No accounts is nothing to say, not "just now".
    @Test func noAccountsIsNothingToSay() {
        #expect(MailboxMerge.oldestSync([], { _ in 1000 }) == nil)
    }
}
