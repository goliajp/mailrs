import Foundation
import Testing

@testable import Mailrs

struct MailCacheTests {
    private func temp() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("mail-cache-test-\(UUID().uuidString)", isDirectory: true)
    }

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

    @Test func roundTripsThePage() {
        let cache = MailCache(directory: temp())
        cache.writeConversations([convo("a"), convo("b")], list: "inbox")
        #expect(cache.readConversations(list: "inbox")?.map(\.threadId) == ["a", "b"])
    }

    @Test func aMissingFileAnswersNil() {
        #expect(MailCache(directory: temp()).readConversations(list: "inbox") == nil)
    }

    @Test func listsDoNotBleedIntoEachOther() {
        let cache = MailCache(directory: temp())
        cache.writeConversations([convo("a")], list: "inbox")
        #expect(cache.readConversations(list: "junk") == nil)
    }

    /// Yesterday's schema answers nil today — and stops being asked.
    @Test func aCorruptFileAnswersNilAndIsDeleted() {
        let dir = temp()
        let cache = MailCache(directory: dir)
        cache.writeConversations([convo("a")], list: "inbox")
        let file = dir.appendingPathComponent("conversations-inbox.json")
        try! Data("not json".utf8).write(to: file)
        #expect(cache.readConversations(list: "inbox") == nil)
        #expect(!FileManager.default.fileExists(atPath: file.path))
    }
}
