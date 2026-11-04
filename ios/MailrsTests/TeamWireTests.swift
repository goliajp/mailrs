import Foundation
import Testing

@testable import Mailrs

/// The same cases as Android's `TeamWireTest`.
@Suite("Team conversation")
struct TeamWireTests {
    @Test func aSharedConvIsTwoNumbers() {
        #expect(TeamWire.Conv("1:7")?.path == "/api/shared/1/7")
        #expect(TeamWire.Conv("") == nil)
        #expect(TeamWire.Conv(nil) == nil)
        #expect(TeamWire.Conv("1") == nil)
        #expect(TeamWire.Conv("a:7") == nil)
        #expect(TeamWire.Conv("1:7:9") == nil)
    }

    @Test func someoneElseWritingIsSaidBeforeWhoIsAssigned() {
        let o = TeamWire.Overview(assignee: "bob@golia.jp", draft: .init(author: "carol@golia.jp"))
        #expect(TeamWire.replyHint(o, me: "me@golia.jp") == .writing("carol"))
    }

    @Test func myOwnDraftFallsThroughToTheAssignee() {
        let o = TeamWire.Overview(assignee: "bob@golia.jp", draft: .init(author: "Me@Golia.jp"))
        #expect(TeamWire.replyHint(o, me: "me@golia.jp") == .assigned("bob"))
    }

    @Test func mineOrNobodySaysNothing() {
        #expect(TeamWire.replyHint(.init(assignee: "me@golia.jp", draft: nil), me: "me@golia.jp") == nil)
        #expect(TeamWire.replyHint(.init(assignee: nil, draft: nil), me: "me@golia.jp") == nil)
        #expect(TeamWire.replyHint(.init(assignee: "", draft: .init(author: "")), me: "me@golia.jp") == nil)
    }
}
