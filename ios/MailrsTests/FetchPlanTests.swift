import Foundation
import Testing

@testable import Mailrs

/// What to ask a folder for, given what is already held.
@Suite struct FetchPlanTests {
    @Test func aFolderNeverReadIsReadWhole() {
        #expect(FetchPlan.decide(mark: nil, serverValidity: 1) == .everything)
        #expect(FetchPlan.everything.range == "1:*")
    }

    /// The next uid, not the last one — asking from the last would
    /// fetch the newest message again on every pass.
    @Test func aFolderReadBeforeAsksForWhatCameAfter() {
        let mark = FolderMark(uidValidity: 7, highestUid: 4390)
        let plan = FetchPlan.decide(mark: mark, serverValidity: 7)
        #expect(plan == .since(uid: 4390))
        #expect(plan.range == "4391:*")
    }

    /// **The one every client gets wrong once.** A changed
    /// `UIDVALIDITY` means uid 4390 is not the message it was, so
    /// "everything after 4390" skips mail or fetches the wrong thing.
    @Test func aRenumberedFolderIsReadFromTheStart() {
        let mark = FolderMark(uidValidity: 7, highestUid: 4390)
        let plan = FetchPlan.decide(mark: mark, serverValidity: 8)
        #expect(plan == .renumbered)
        #expect(plan.range == "1:*", "a renumbered folder was resumed from a stale uid")
    }

    /// A mark that was written before anything was read is not a
    /// resume point.
    @Test func aMarkWithNoUidIsNotAResumePoint() {
        let mark = FolderMark(uidValidity: 7, highestUid: 0)
        #expect(FetchPlan.decide(mark: mark, serverValidity: 7) == .everything)
    }

    /// The validity travels **with** the uid. Stored apart they drift,
    /// and a uid without the validity that issued it means nothing.
    @Test func theMarkCarriesBothOrNeither() throws {
        let mark = FolderMark(uidValidity: 7, highestUid: 4390)
        let back = try JSONDecoder().decode(
            FolderMark.self, from: try JSONEncoder().encode(mark))
        #expect(back == mark)
    }
}
