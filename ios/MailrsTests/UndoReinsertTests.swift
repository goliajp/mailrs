import Foundation
import Testing

@testable import Mailrs

/// The undo slot's one piece of arithmetic: rows going back to the
/// positions they left. Ascending insert order is what makes stored
/// indices mean what they meant — each insert restores the coordinate
/// system the next was recorded in.
struct UndoReinsertTests {
    private func convo(_ id: String) -> Wire.Conversation {
        let json = """
        {"thread_id": "\(id)", "subject": "s", "participants": [],
         "message_count": 1, "unread_count": 0, "last_date": 1,
         "category": "inbox", "flagged": false, "snippet": "",
         "pinned": false, "archived": false, "importance_level": "normal",
         "importance_score": 0, "requires_action": false,
         "received_count": 1, "sent_count": 0}
        """
        return try! JSONDecoder().decode(Wire.Conversation.self, from: Data(json.utf8))
    }

    @Test func rowsReturnToTheirOriginalPositions() {
        // a b c d e; b(1) and d(3) archived leaves a c e
        let remaining = [convo("a"), convo("c"), convo("e")]
        let rows = [
            Session.UndoableRow(conversation: convo("d"), index: 3),
            Session.UndoableRow(conversation: convo("b"), index: 1),
        ]
        let restored = Session.reinserted(rows, into: remaining)
        #expect(restored.map(\.threadId) == ["a", "b", "c", "d", "e"])
    }

    @Test func anIndexPastTheEndClampsToAppend() {
        let restored = Session.reinserted(
            [Session.UndoableRow(conversation: convo("x"), index: 99)],
            into: [convo("a")]
        )
        #expect(restored.map(\.threadId) == ["a", "x"])
    }

    @Test func emptyListRestoresCompletely() {
        let rows = [
            Session.UndoableRow(conversation: convo("a"), index: 0),
            Session.UndoableRow(conversation: convo("b"), index: 1),
        ]
        let restored = Session.reinserted(rows, into: [])
        #expect(restored.map(\.threadId) == ["a", "b"])
    }
}
