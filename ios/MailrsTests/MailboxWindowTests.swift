import Testing

@testable import Mailrs

/// The two things the end of the list can offer.
///
/// One is a `LIMIT` and costs nothing; the other is a network round
/// trip. They were one button once, and the slow one ran whenever the
/// fast one would have done.
@Suite struct MailboxWindowTests {
    @Test func aFullWindowIsEvidenceThereMayBeMore() {
        #expect(MailboxWindow.moreHeld(returned: 200, asked: 200))
    }

    // The direction that matters: short is **proof** there is no more,
    // and it is what stops the list growing forever.
    @Test func aShortWindowIsProofThereIsNot() {
        #expect(!MailboxWindow.moreHeld(returned: 199, asked: 200))
        #expect(!MailboxWindow.moreHeld(returned: 0, asked: 200))
    }

    @Test func theSlowActionIsNotOfferedWhileTheFastOneWouldDo() {
        #expect(
            !MailboxWindow.offersEarlier(moreHeld: true, shownCount: 200, searching: false))
        #expect(
            MailboxWindow.offersEarlier(moreHeld: false, shownCount: 200, searching: false))
    }

    // "Earlier" than nothing has no anchor to reach back from — the
    // ordinary pass is what gives a folder one.
    @Test func nothingToBeEarlierThanIsNotOffered() {
        #expect(
            !MailboxWindow.offersEarlier(moreHeld: false, shownCount: 0, searching: false))
    }

    // A fetch against a filtered list brings back mail that will not be
    // shown, and looks like it did nothing.
    @Test func notWhileSearching() {
        #expect(
            !MailboxWindow.offersEarlier(moreHeld: false, shownCount: 200, searching: true))
    }
}
