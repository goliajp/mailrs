import Foundation
import Testing

@testable import Mailrs

private func conversation(_ id: String, _ date: Int64) -> Wire.Conversation {
    let json = Data("""
    {"thread_id":"\(id)","subject":"s","participants":["a@b.jp"],"message_count":1,
     "unread_count":0,"last_date":\(date),"category":"inbox","flagged":false,
     "snippet":"","pinned":false,"archived":false,"importance_level":"normal",
     "importance_score":0,"requires_action":false,"received_count":1,"sent_count":0}
    """.utf8)
    // Decoded rather than constructed so the fixture stays honest about
    // the wire shape.
    return try! JSONDecoder().decode(Wire.Conversation.self, from: json)
}

struct ThreadPageTests {
    /// The server filters `latest_date < before_ts`, so asking for the
    /// oldest row's own second would drop its siblings. One past it
    /// re-requests the boundary instead.
    @Test func asksPastTheBoundarySecond() {
        let rows = [conversation("a", 200), conversation("b", 100)]
        #expect(ThreadPage.nextBefore(after: rows) == 101)
    }

    @Test func hasNoCursorForAnEmptyList() {
        #expect(ThreadPage.nextBefore(after: []) == nil)
    }

    @Test func appendsNewRowsAndKeepsOrder() {
        let held = [conversation("a", 300), conversation("b", 200)]
        let page = [conversation("c", 100)]
        let merged = ThreadPage.merge(held, with: page)
        #expect(merged.rows.map(\.threadId) == ["a", "b", "c"])
        #expect(merged.progressed)
    }

    /// The overlap the boundary-second request buys, discarded.
    @Test func dropsRowsAlreadyHeld() {
        let held = [conversation("a", 300), conversation("b", 200)]
        let page = [conversation("b", 200), conversation("c", 200)]
        let merged = ThreadPage.merge(held, with: page)
        #expect(merged.rows.map(\.threadId) == ["a", "b", "c"])
        #expect(merged.progressed)
    }

    /// The condition that stops the loop. A page can come back full of
    /// rows already on screen — that is the cost of re-requesting the
    /// boundary — and paging on "was it full?" would ask for the same
    /// second forever.
    @Test func reportsNoProgressWhenThePageIsAllOldRows() {
        let held = [conversation("a", 300), conversation("b", 300)]
        let merged = ThreadPage.merge(held, with: [conversation("a", 300), conversation("b", 300)])
        #expect(merged.rows.count == 2)
        #expect(!merged.progressed)
    }

    @Test func reportsNoProgressForAnEmptyPage() {
        let held = [conversation("a", 300)]
        #expect(!ThreadPage.merge(held, with: []).progressed)
    }
}
