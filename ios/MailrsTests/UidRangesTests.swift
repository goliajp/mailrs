import Testing

@testable import Mailrs

/// Turning a list of uids into something a server will accept.
@Suite struct UidRangesTests {
    /// Consecutive uids are a range, which is what they nearly always
    /// are.
    @Test func runsCollapse() {
        #expect(UidRanges.collapse([1, 2, 3, 7, 8, 20]) == "1:3,7:8,20")
        #expect(UidRanges.collapse([5]) == "5")
        #expect(UidRanges.collapse([]) == "")
    }

    /// Sorted first: a set has no order, and a server reading `20,1:3`
    /// gets a valid but pointlessly awkward sequence.
    @Test func orderDoesNotMatterToTheCaller() {
        #expect(UidRanges.collapse([20, 2, 1, 3]) == "1:3,20")
        // And a repeat is not two uids.
        #expect(UidRanges.collapse([1, 2, 2, 1]) == "1:2")
    }

    /// **The mailbox that most needs its flags refreshed is the one
    /// where a naive command stops working**: five thousand uids named
    /// one by one is a line tens of kilobytes long, and servers refuse
    /// over-long lines.
    @Test func aLongSparseListIsSplitIntoCommandsAServerWillTake() {
        // Every other uid, so nothing collapses.
        let sparse = stride(from: UInt32(1), through: 4000, by: 2).map { $0 }
        let batches = UidRanges.batches(sparse)
        #expect(batches.count > 1, "nothing was split")
        for batch in batches { #expect(batch.count <= UidRanges.maxChars) }
        // Nothing was lost or invented.
        #expect(batches.joined(separator: ",") == UidRanges.collapse(sparse))
    }

    /// **Split on whole runs, never inside one.** Half of `1:3` is not
    /// a range, and a server would read whatever the halves happen to
    /// spell.
    @Test func aRangeIsNeverCutInHalf() {
        let sparse = stride(from: UInt32(1), through: 4000, by: 2).map { $0 }
        for batch in UidRanges.batches(sparse, maxChars: 40) {
            #expect(!batch.hasPrefix(":") && !batch.hasSuffix(":"))
            for run in batch.split(separator: ",") {
                let parts = run.split(separator: ":", omittingEmptySubsequences: false)
                #expect(parts.count <= 2)
                #expect(parts.allSatisfy { !$0.isEmpty })
            }
        }
    }

    /// A short list is one command, and an empty one is no command.
    @Test func shortAndEmpty() {
        #expect(UidRanges.batches(Array(UInt32(1)...10)) == ["1:10"])
        #expect(UidRanges.batches([]).isEmpty)
    }
}
