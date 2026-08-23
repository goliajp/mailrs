import Foundation
import Testing

@testable import Mailrs

/// What to ask a folder for, given what is already held.
@Suite struct FetchPlanTests {
    /// **A window, not the whole folder.** A first sync of a mailbox
    /// with fifty thousand messages would fetch fifty thousand header
    /// blocks — hundreds of megabytes, many minutes, and a row list
    /// far past what the device stores in one go.
    @Test func aFolderNeverReadIsReadFromItsEnd() {
        let plan = FetchPlan.decide(
            mark: nil, serverValidity: 1, exists: 50_000, window: 500)
        #expect(plan.range == "49501:*")
    }

    /// **By position, not by uid.** "The last five hundred messages"
    /// is what a sequence number means; there is no uid arithmetic
    /// that says it, because uids have gaps wherever anything was
    /// deleted. `UID FETCH 1:500` and `FETCH 1:500` are different
    /// questions.
    @Test func aFirstPassCountsPositionsAndAResumeCountsUids() {
        #expect(FetchPlan.decide(mark: nil, serverValidity: 1, exists: 50_000).byUid == false)
        #expect(
            FetchPlan.decide(
                mark: FolderMark(uidValidity: 7, highestUid: 4390), serverValidity: 7
            ).byUid)
    }

    /// A folder smaller than the window is read whole.
    @Test func aSmallFolderIsReadWhole() {
        #expect(
            FetchPlan.decide(mark: nil, serverValidity: 1, exists: 12, window: 500).range
                == "1:*")
        #expect(
            FetchPlan.decide(mark: nil, serverValidity: 1, exists: 0, window: 500).range
                == "1:*")
    }

    /// The next uid, not the last one — asking from the last would
    /// fetch the newest message again on every pass.
    @Test func aFolderReadBeforeAsksForWhatCameAfter() {
        let plan = FetchPlan.decide(
            mark: FolderMark(uidValidity: 7, highestUid: 4390), serverValidity: 7)
        #expect(plan == .since(uid: 4390))
        #expect(plan.range == "4391:*")
    }

    /// **The one every client gets wrong once.** A changed
    /// `UIDVALIDITY` means uid 4390 is not the message it was, so
    /// "everything after 4390" skips mail or fetches the wrong thing —
    /// and the folder is read from its end again, by position.
    @Test func aRenumberedFolderIsReadAgainFromItsEnd() {
        let plan = FetchPlan.decide(
            mark: FolderMark(uidValidity: 7, highestUid: 4390), serverValidity: 8,
            exists: 50_000, window: 500)
        if case .renumbered = plan {} else { Issue.record("not renumbered: \(plan)") }
        #expect(plan.range == "49501:*", "a renumbered folder was resumed from a stale uid")
        #expect(plan.byUid == false, "a renumbered folder was asked by uid")
    }

    @Test func aMarkWithNoUidIsNotAResumePoint() {
        let plan = FetchPlan.decide(
            mark: FolderMark(uidValidity: 7, highestUid: 0), serverValidity: 7, exists: 12)
        if case .newest = plan {} else { Issue.record("not newest: \(plan)") }
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
