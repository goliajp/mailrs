import Foundation
import Testing

@testable import Mailrs

private func conversation(_ id: String, _ date: Int64, cursor: String? = nil) -> Wire.Conversation {
    var cursorField = ""
    if let cursor { cursorField = #","cursor":"\#(cursor)""# }
    let json = Data("""
    {"thread_id":"\(id)","subject":"s","participants":["a@b.jp"],"message_count":1,
     "unread_count":0,"last_date":\(date),"category":"inbox","flagged":false,
     "snippet":"","pinned":false,"archived":false,"importance_level":"normal",
     "importance_score":0,"requires_action":false,"received_count":1,"sent_count":0\(cursorField)}
    """.utf8)
    // Decoded rather than constructed so the fixture stays honest about
    // the wire shape.
    return try! JSONDecoder().decode(Wire.Conversation.self, from: json)
}

struct ThreadPageTests {
    /// The next page starts after the last row, as the server marked it.
    @Test func asksForThePageAfterTheLastRowsCursor() {
        let rows = [conversation("a", 200, cursor: "200:7"), conversation("b", 100, cursor: "100:3")]
        #expect(ThreadPage.next(after: rows) == .cursor("100:3"))
        #expect(ThreadPage.queryItems(ThreadPage.next(after: rows))
            == [URLQueryItem(name: "cursor", value: "100:3")])
    }

    /// A row cached before the server stamped a cursor pages the old way:
    /// `latest_date < before_ts`, one second past the oldest row so its
    /// siblings are re-requested rather than dropped.
    @Test func withoutACursorAsksPastTheBoundarySecond() {
        let rows = [conversation("a", 200), conversation("b", 100)]
        #expect(ThreadPage.next(after: rows) == .before(101))
        #expect(ThreadPage.queryItems(.before(101))
            == [URLQueryItem(name: "before_ts", value: "101")])
    }

    @Test func hasNoNextPageForAnEmptyList() {
        #expect(ThreadPage.next(after: []) == nil)
        #expect(ThreadPage.queryItems(nil).isEmpty)
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
