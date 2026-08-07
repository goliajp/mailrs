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

    /// `crates/webapi/src/handlers/conversation_body.rs` —
    /// `ThreadMessageResponse`, also a bare array. The struct has forty
    /// fields; this client reads nine and must ignore the rest rather
    /// than failing to decode because an analysis field it never uses
    /// changed shape.
    @Test func decodesThreadMessagesAndIgnoresTheFieldsItDoesNotUse() throws {
        let json = Data("""
        [{"uid":7,"sender":"alice@example.com","sender_trust":"verified",
          "recipients":"me@golia.jp","subject":"Q3","flags":0,
          "internal_date":1754400000,"message_id":"<m1@x>",
          "text_body":"hello","html_body":"<p>hello</p>","attachments":[],
          "category":"inbox","risk_score":0,"risk_reason":"","summary":"",
          "people":{},"dates":{},"amounts":{},"action_items":[],
          "ai_analyzed":false,"importance_level":"normal","importance_score":0.1,
          "is_bulk_sender":false,"has_tracking_pixel":false,"requires_action":false,
          "sender_intent":""}]
        """.utf8)

        let messages = try JSONDecoder().decode([Wire.Message].self, from: json)
        #expect(messages.count == 1)
        #expect(messages[0].uid == 7)
        #expect(messages[0].senderTrust == "verified")
        #expect(messages[0].htmlBody == "<p>hello</p>")
    }

    /// `cc`, `text_body` and `html_body` are all optional on the wire —
    /// `cc` is skipped entirely when absent, and a message can genuinely
    /// have no HTML part.
    @Test func decodesAMessageWithNoHtmlPartAndNoCc() throws {
        let json = Data("""
        [{"uid":1,"sender":"a@b.jp","sender_trust":"","recipients":"me@golia.jp",
          "subject":"","flags":0,"internal_date":1,"message_id":"<x>",
          "text_body":"plain","html_body":null,"attachments":[],"category":"inbox",
          "risk_score":0,"risk_reason":"","summary":"","people":{},"dates":{},
          "amounts":{},"action_items":[],"ai_analyzed":false,
          "importance_level":"normal","importance_score":0,"is_bulk_sender":false,
          "has_tracking_pixel":false,"requires_action":false,"sender_intent":""}]
        """.utf8)

        let messages = try JSONDecoder().decode([Wire.Message].self, from: json)
        #expect(messages[0].htmlBody == nil)
        #expect(messages[0].textBody == "plain")
    }

    /// Both threading fields, every time.
    ///
    /// `compose.rs` marks each of them `#[serde(default)]`, so omitting
    /// one is not an error — it is an empty value the server accepts and
    /// a reply that arrives detached from its thread. The handler's own
    /// comment records that happening on 2026-07-30. This is the
    /// assertion that keeps the pair together.
    @Test func sendsBothThreadingFieldsOnAReply() throws {
        let request = Wire.SendRequest(
            to: ["alice@example.com"], cc: [], subject: "Re: Q3", body: "Noted.",
            inReplyTo: "<m1@x>", replyToThreadId: "t1",
            forwardMessageId: nil, forwardAttachmentsFrom: nil
        )
        let text = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
        #expect(text.contains("\"in_reply_to\":\"<m1@x>\""))
        #expect(text.contains("\"reply_to_thread_id\":\"t1\""))
        // Optionals must vanish when nil, not encode as null — the
        // handler treats a present-but-null forward differently from an
        // absent one only by accident of serde defaults; absent is the
        // shape the web sends.
        #expect(!text.contains("forward_message_id"))
    }

    /// A forward carries the reference and neither threading field.
    @Test func aForwardCarriesTheReferenceAndNoThreading() throws {
        let request = Wire.SendRequest(
            to: ["x@example.com"], cc: [], subject: "Fwd: Q3", body: "FYI.",
            inReplyTo: nil, replyToThreadId: nil,
            forwardMessageId: "<m2@x>", forwardAttachmentsFrom: 2
        )
        let text = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
        #expect(text.contains("\"forward_message_id\":\"<m2@x>\""))
        #expect(text.contains("\"forward_attachments_from\":2"))
        #expect(!text.contains("in_reply_to"))
        #expect(!text.contains("reply_to_thread_id"))
    }

    /// `crates/webapi/src/handlers/compose.rs` — `SendResponse`. The
    /// handler answers 200 with `success: false` for a message it took
    /// but could not queue, so the status code is not the whole answer.
    @Test func decodesARefusedSend() throws {
        let json = Data(#"{"message_id":"","success":false,"message":"queue unavailable"}"#.utf8)
        let response = try JSONDecoder().decode(Wire.SendResponse.self, from: json)
        #expect(!response.success)
        #expect(response.message == "queue unavailable")
    }

    /// Attachments as `conversation_body.rs` builds them: filename,
    /// content_type, size, and `content_id` only on inline parts.
    ///
    /// No `index` — the position in the array is the index, which is how
    /// `get_attachment` resolves it (`attachments.get(index)`).
    @Test func decodesAttachmentsWithoutAnIndexField() throws {
        let json = Data("""
        [{"uid":7,"sender":"a@b.jp","sender_trust":"","recipients":"me@golia.jp",
          "subject":"","flags":0,"internal_date":1,"message_id":"<x>",
          "text_body":null,"html_body":null,"category":"inbox","risk_score":0,
          "risk_reason":"","summary":"","people":{},"dates":{},"amounts":{},
          "action_items":[],"ai_analyzed":false,"importance_level":"normal",
          "importance_score":0,"is_bulk_sender":false,"has_tracking_pixel":false,
          "requires_action":false,"sender_intent":"",
          "attachments":[
            {"filename":"請求書.pdf","content_type":"application/pdf","size":12345},
            {"filename":"logo.png","content_type":"image/png","size":900,
             "content_id":"logo@example.com"}]}]
        """.utf8)

        let messages = try JSONDecoder().decode([Wire.Message].self, from: json)
        let attachments = messages[0].attachments
        #expect(attachments.count == 2)
        #expect(attachments[0].filename == "請求書.pdf")
        #expect(attachments[0].contentId == nil)
        #expect(attachments[1].contentId == "logo@example.com")
    }

    /// A message with no attachments still decodes — the field is always
    /// present on the wire, but as an empty array.
    @Test func decodesAMessageWithNoAttachments() throws {
        let json = Data("""
        [{"uid":1,"sender":"a@b.jp","sender_trust":"","recipients":"me@golia.jp",
          "subject":"","flags":0,"internal_date":1,"message_id":"<x>",
          "text_body":"hi","html_body":null,"attachments":[],"category":"inbox",
          "risk_score":0,"risk_reason":"","summary":"","people":{},"dates":{},
          "amounts":{},"action_items":[],"ai_analyzed":false,
          "importance_level":"normal","importance_score":0,"is_bulk_sender":false,
          "has_tracking_pixel":false,"requires_action":false,"sender_intent":""}]
        """.utf8)
        #expect(try JSONDecoder().decode([Wire.Message].self, from: json)[0].attachments.isEmpty)
    }

    @Test func encodesLoginWithSnakeCaseTotp() throws {
        let body = Wire.LoginRequest(address: "a@golia.jp", password: "pw", totpCode: "123456")
        let encoded = try JSONEncoder().encode(body)
        let text = String(decoding: encoded, as: UTF8.self)
        #expect(text.contains("\"totp_code\":\"123456\""))
    }
}
