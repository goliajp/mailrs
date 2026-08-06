import Foundation
import Testing

@testable import Mailrs

/// The wire types are checked against bytes the Rust handlers produce,
/// not against bytes shaped like the Swift types.
///
/// A fixture written to match the model passes whatever the model says
/// and proves nothing — nine of the web client's schemas drifted from
/// the backend exactly that way, and every one of them had a green test
/// (`.claude/rules/frontend/wire-schema-verification.md`).
struct WireTests {
    /// `crates/webapi/src/handlers/conversations.rs` returns
    /// `Json<Vec<ConversationResponse>>` — an array, with no envelope.
    @Test func decodesTheConversationListAsABareArray() throws {
        let json = Data("""
        [{"thread_id":"t1","subject":"Quarterly report","participants":["alice@example.com"],
          "message_count":3,"unread_count":2,"last_date":1754400000,"category":"inbox",
          "flagged":false,"snippet":"Please review","pinned":false,"archived":false,
          "importance_level":"normal","importance_score":0.5,"requires_action":false,
          "received_count":3,"sent_count":0}]
        """.utf8)

        let list = try JSONDecoder().decode([Wire.Conversation].self, from: json)
        #expect(list.count == 1)
        #expect(list[0].threadId == "t1")
        #expect(list[0].unreadCount == 2)
        #expect(list[0].participants == ["alice@example.com"])
    }

    /// An envelope must NOT decode. This is the assertion that would have
    /// caught writing the client against the stub shape rather than the
    /// handler — which is precisely the mistake made while scaffolding.
    @Test func rejectsAnEnvelopeShape() {
        let json = Data(#"{"items":[],"has_more":false,"next_cursor":null}"#.utf8)
        #expect(throws: (any Error).self) {
            try JSONDecoder().decode([Wire.Conversation].self, from: json)
        }
    }

    /// `crates/webapi/src/handlers/auth/session.rs` — `LoginResponse`.
    @Test func decodesLogin() throws {
        let json = Data("""
        {"address":"a@golia.jp","display_name":"A","permissions":["admin.accounts"],"token":"deadbeef"}
        """.utf8)
        let login = try JSONDecoder().decode(Wire.LoginResponse.self, from: json)
        #expect(login.displayName == "A")
        #expect(login.token == "deadbeef")
    }

    /// The handler short-circuits with this before issuing a session, so
    /// it has to be recognised rather than surfacing as a decode failure.
    @Test func recognisesTheTotpChallenge() throws {
        let json = Data(#"{"requires_totp":true}"#.utf8)
        let challenge = try JSONDecoder().decode(Wire.TotpChallenge.self, from: json)
        #expect(challenge.requiresTotp)
    }

    @Test func encodesLoginWithSnakeCaseTotp() throws {
        let body = Wire.LoginRequest(address: "a@golia.jp", password: "pw", totpCode: "123456")
        let encoded = try JSONEncoder().encode(body)
        let text = String(decoding: encoded, as: UTF8.self)
        #expect(text.contains("\"totp_code\":\"123456\""))
    }
}
