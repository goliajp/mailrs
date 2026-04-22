import Testing
import Foundation
@testable import Mailrs

@Suite("ThreadMessage decoding")
struct ThreadMessageTests {
    @Test("decodes typical thread message")
    func decodeTypical() throws {
        let json = Data("""
        {
          "uid": 42,
          "message_id": "<msg-1@example.com>",
          "subject": "Hello",
          "sender": "Alice <alice@example.com>",
          "recipients": "me@example.com",
          "internal_date": 1700000000,
          "flags": 5,
          "html_body": "<p>Hello</p>",
          "text_body": "Hello",
          "clean_text": "Hello",
          "new_content": null,
          "attachments": [
            {"filename": "a.pdf", "content_type": "application/pdf", "size": 1024}
          ],
          "category": "personal",
          "importance_level": "medium",
          "importance_score": 0.5,
          "requires_action": false,
          "risk_score": 0.1,
          "risk_reason": "",
          "summary": "A short hello",
          "sender_intent": "informational",
          "has_tracking_pixel": false,
          "is_bulk_sender": false,
          "ai_analyzed": true,
          "bimi_logo_url": null,
          "action_deadline": null,
          "action_items": ["reply"]
        }
        """.utf8)

        let m = try JSONCoders.decoder.decode(ThreadMessage.self, from: json)
        #expect(m.uid == 42)
        #expect(m.subject == "Hello")
        #expect(m.attachments.count == 1)
        #expect(m.attachments[0].filename == "a.pdf")
        // flags=5 -> FLAG_SEEN (1) + FLAG_FLAGGED (4)
        #expect(m.isSeen == true)
        #expect(m.isFlagged == true)
        #expect(m.isAnswered == false)
    }

    @Test("decodes without optional structured_data")
    func decodeMinimal() throws {
        let json = Data("""
        {
          "uid": 1, "message_id": "m", "subject": "", "sender": "x",
          "recipients": "y", "internal_date": 0, "flags": 0,
          "html_body": null, "text_body": null, "clean_text": null, "new_content": null,
          "attachments": [], "category": "general", "importance_level": "low",
          "importance_score": 0, "requires_action": false, "risk_score": 0,
          "risk_reason": "", "summary": "", "sender_intent": "",
          "has_tracking_pixel": false, "is_bulk_sender": false
        }
        """.utf8)
        let m = try JSONCoders.decoder.decode(ThreadMessage.self, from: json)
        #expect(m.structured_data == nil)
        #expect(m.preferredBodyText.isEmpty)
    }
}
