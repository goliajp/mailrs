import Testing
import Foundation
@testable import Mailrs

@Suite("MailEvent decoding")
struct MailEventTests {
    @Test("decodes NewMessage event")
    func newMessage() throws {
        let json = Data("""
        {
          "type": "NewMessage",
          "sender": "Alice <a@x.com>",
          "snippet": "hi",
          "subject": "Hello",
          "thread_id": "t-1",
          "user": "me@x.com"
        }
        """.utf8)

        let event = try JSONCoders.decoder.decode(MailEvent.self, from: json)
        guard case .newMessage(let e) = event else {
            Issue.record("expected newMessage, got \(event)")
            return
        }
        #expect(e.sender == "Alice <a@x.com>")
        #expect(e.subject == "Hello")
        #expect(e.user == "me@x.com")
        #expect(e.thread_id == "t-1")
    }

    @Test("unknown type decodes as .other, never throws")
    func unknownType() throws {
        let json = Data(#"{"type": "ConnectionOpened", "id": 1}"#.utf8)
        let event = try JSONCoders.decoder.decode(MailEvent.self, from: json)
        if case .other(let type) = event {
            #expect(type == "ConnectionOpened")
        } else {
            Issue.record("expected .other")
        }
    }

    @Test("missing type field decodes as .other with empty type")
    func missingType() throws {
        let event = try JSONCoders.decoder.decode(MailEvent.self, from: Data("{}".utf8))
        if case .other(let type) = event {
            #expect(type == "")
        } else {
            Issue.record("expected .other")
        }
    }
}
