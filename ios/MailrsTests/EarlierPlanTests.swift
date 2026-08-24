import Testing

@testable import Mailrs

/// What to ask for when somebody wants the mail before what they have.
@Suite struct EarlierPlanTests {
    /// The range stops one below what is already held — not at it.
    @Test func itAsksForWhatIsBelowTheLowestHeld() {
        #expect(EarlierPlan.decide(lowestHeldUid: 1001, span: 200).range == "801:1000")
    }

    /// And never below uid 1, which does not exist.
    @Test func itDoesNotReachPastTheBeginning() {
        #expect(EarlierPlan.decide(lowestHeldUid: 50, span: 200).range == "1:49")
    }

    /// **Uid 1 held means the folder is exhausted.** There is no uid
    /// below it, and asking would be a round trip whose answer is
    /// known.
    @Test func holdingTheFirstMessageMeansThereIsNothingOlder() {
        #expect(EarlierPlan.decide(lowestHeldUid: 1).range == nil)
        #expect(EarlierPlan.decide(lowestHeldUid: 0).range == nil)
    }

    /// **Uids leave gaps wherever something was deleted**, so a span
    /// of 200 may hold five messages. Widening is what stops somebody
    /// tapping "earlier" five times to see one message.
    @Test func aThinAnswerWidensTheNextSpan() {
        #expect(EarlierPlan.nextSpan(200, returned: 2) == 800)
        #expect(EarlierPlan.nextSpan(800, returned: 0) == 3200)
    }

    /// A full answer asked about the right amount.
    @Test func aFullAnswerKeepsTheSpan() {
        #expect(EarlierPlan.nextSpan(200, returned: 200) == 200)
        #expect(EarlierPlan.nextSpan(200, returned: EarlierPlan.thin) == 200)
    }

    /// And the widening stops, or one tap becomes its own problem.
    @Test func theSpanHasACeiling() {
        #expect(EarlierPlan.nextSpan(EarlierPlan.maxSpan, returned: 0) == EarlierPlan.maxSpan)
        #expect(EarlierPlan.nextSpan(2000, returned: 0) <= EarlierPlan.maxSpan)
    }

    /// **Finished is not the same as empty.** A range that is all gaps
    /// returns nothing and there may be plenty below it; the folder is
    /// finished when the range reached uid 1.
    @Test func exhaustedMeansTheRangeReachedTheBeginning() {
        #expect(EarlierPlan.exhausted(EarlierPlan.decide(lowestHeldUid: 50, span: 200)))
        #expect(!EarlierPlan.exhausted(EarlierPlan.decide(lowestHeldUid: 1001, span: 200)))
    }

    // At the ceiling the cap drops the oldest rows and this fetches
    // exactly those, so the two undo each other. Refusing is the
    // honest answer; fetching-and-discarding looks like it worked.
    @Test func aFullDeviceIsAskedBeforeTheNetworkIs() {
        #expect(EarlierPlan.atCeiling(held: 100, ceiling: 100))
        #expect(EarlierPlan.atCeiling(held: 101, ceiling: 100))
        #expect(!EarlierPlan.atCeiling(held: 99, ceiling: 100))
    }

    // The ceiling has to be far above one span, or the button works
    // once and then stops for a reason nobody can see.
    @Test func theCeilingLeavesRoomForManySpans() {
        #expect(MailboxApply.perAccount >= EarlierPlan.firstSpan * 20)
    }
}
