import Foundation
import Testing

@testable import Mailrs

/// What the message menu's Block knows without the server: an address on
/// one list at most, the question that names the allowed list only when
/// the address is on it, and the undo bar that says what was blocked.
@MainActor struct BlockSenderTests {
    @Test func anAddressIsOnOneListAtMost() {
        var lists = SenderLists(blocked: [], allowed: ["a@x.example"])
        lists.block("a@x.example")
        #expect(lists.blocked == ["a@x.example"])
        #expect(lists.allowed.isEmpty)
        lists.allow("a@x.example")
        #expect(lists.blocked.isEmpty)
        #expect(lists.allowed == ["a@x.example"])
    }

    @Test func theQuestionMentionsTheAllowedListOnlyWhenTheAddressIsOnIt() {
        let plain = BlockSenderConfirm.message("a@x.example", allowed: false)
        let allowed = BlockSenderConfirm.message("a@x.example", allowed: true)
        #expect(!plain.contains("a@x.example"))
        #expect(allowed.hasPrefix(plain))
        #expect(allowed.contains("a@x.example"))
    }

    @Test func theUndoBarSaysWhoWasBlocked() {
        let undo = Session.PendingUndo(
            rows: [], action: .markJunk,
            blocked: BlockedSender(threadId: "t1", address: "a@x.example", wasAllowed: false)
        )
        // LocalizedStringKey does not compare its arguments, so read its
        // description: the catalog key, and the address it is filled with
        let label = String(describing: UndoBar.label(undo))
        #expect(label.contains("Blocked %@"))
        #expect(label.contains("a@x.example"))
        let plain = String(describing: UndoBar.label(Session.PendingUndo(rows: [], action: .markJunk)))
        #expect(!plain.contains("Blocked %@"))
    }

    /// The field the handler writes is `was_allowed`; a missed key would
    /// decode as an error and the block would look like it failed.
    @Test func theAnswerDecodesTheServersFieldNames() throws {
        let data = Data(#"{"address":"a@x.example","was_allowed":true}"#.utf8)
        let answer = try JSONDecoder().decode(Wire.BlockSenderResponse.self, from: data)
        #expect(answer == Wire.BlockSenderResponse(address: "a@x.example", wasAllowed: true))
    }

    @Test func theUndoSendsAllowBack() throws {
        let body = try JSONEncoder().encode(Wire.UnblockSenderRequest(address: "a@x.example", allow: true))
        let json = try JSONSerialization.jsonObject(with: body) as? [String: Any]
        #expect(json?["address"] as? String == "a@x.example")
        #expect(json?["allow"] as? Bool == true)
    }
}
