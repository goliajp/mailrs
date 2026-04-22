import Testing
import Foundation
@testable import Mailrs

@Suite("ConversationSummary decoding")
struct ConversationSummaryTests {
    @Test("decodes a real server-shape payload")
    func decodeTypical() throws {
        let json = Data("""
        {
          "thread_id": "abc-123",
          "subject": "Your invoice",
          "snippet": "Please find attached",
          "last_sender": "Alice <alice@example.com>",
          "last_date": 1700000000,
          "message_count": 3,
          "unread_count": 1,
          "participants": ["alice@example.com", "bob@example.com"],
          "category": "personal",
          "importance_level": "medium",
          "importance_score": 0.42,
          "flagged": false,
          "pinned": true,
          "archived": false,
          "requires_action": false
        }
        """.utf8)

        let c = try JSONCoders.decoder.decode(ConversationSummary.self, from: json)
        #expect(c.id == "abc-123")
        #expect(c.subject == "Your invoice")
        #expect(c.last_date.timeIntervalSince1970 == 1700000000)
        #expect(c.isUnread == true)
        #expect(c.pinned == true)
        #expect(c.participants.count == 2)
        #expect(c.lastDateUnixSeconds == 1700000000)
    }

    @Test("isUnread reflects unread_count")
    func unreadFlag() {
        var base = try! JSONCoders.decoder.decode(ConversationSummary.self, from: Data("""
        {
          "thread_id": "t", "subject": "", "snippet": "", "last_sender": "x",
          "last_date": 0, "message_count": 1, "unread_count": 0,
          "participants": [], "category": "general", "importance_level": "low",
          "importance_score": 0, "flagged": false, "pinned": false,
          "archived": false, "requires_action": false
        }
        """.utf8))
        #expect(base.isUnread == false)
        base = ConversationSummary(
            thread_id: base.thread_id,
            subject: base.subject,
            snippet: base.snippet,
            last_sender: base.last_sender,
            last_date: base.last_date,
            message_count: base.message_count,
            unread_count: 5,
            participants: base.participants,
            category: base.category,
            importance_level: base.importance_level,
            importance_score: base.importance_score,
            flagged: base.flagged,
            pinned: base.pinned,
            archived: base.archived,
            requires_action: base.requires_action
        )
        #expect(base.isUnread == true)
    }
}
