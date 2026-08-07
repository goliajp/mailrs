import Foundation
import Testing

@testable import Mailrs

/// Mirrors `web/src/components/send-list/__tests__` around `joinSends` —
/// the two clients answer "what happened to my mail" with one rule.
struct SendJoinTests {
    private func message(_ id: String, thread: String = "t1", date: Int64 = 100) -> Wire.SentMessage {
        let json = """
        {"uid":7,"message_id":"\(id)","thread_id":"\(thread)","to":"a@b.jp",
         "subject":"S","internal_date":\(date)}
        """
        return try! JSONDecoder().decode(Wire.SentMessage.self, from: Data(json.utf8))
    }

    private func send(
        _ id: String, status: String, resentFrom: String? = nil, created: Int64 = 100
    ) -> Wire.Send {
        let resent = resentFrom.map { "\"\($0)\"" } ?? "null"
        let json = """
        {"send_id":"\(id)","thread_id":"t1","subject":"S","to":["a@b.jp"],
         "created_at":\(created),"status":"\(status)","can_resend":false,
         "resent_from":\(resent),"recipients":[]}
        """
        return try! JSONDecoder().decode(Wire.Send.self, from: Data(json.utf8))
    }

    /// The join must survive one side carrying angle brackets — if it
    /// fails it fails silently, as every row losing its status.
    @Test func joinsAcrossBracketAndCaseDifferences() {
        let rows = SendJoin.join(
            messages: [message("<MsgID@Golia.JP>")],
            sends: [send("msgid@golia.jp", status: "failed")]
        )
        #expect(rows.count == 1)
        #expect(rows[0].status == "failed")
    }

    /// Mail that predates the projection says nothing rather than
    /// claiming delivery.
    @Test func absentProjectionMeansNoStatus() {
        let rows = SendJoin.join(messages: [message("m1@x")], sends: [])
        #expect(rows[0].status == nil)
    }

    /// A send the maildir sweep has not filed is still a row — the send
    /// that just left is exactly the one being looked for.
    @Test func unfiledSendsAppear() {
        let rows = SendJoin.join(messages: [], sends: [send("m2@x", status: "sending")])
        #expect(rows.count == 1)
        #expect(rows[0].status == "sending")
        #expect(rows[0].uid == nil)
    }

    /// A resend chain reports the newest attempt's status against the
    /// original message.
    @Test func resendChainKeepsTheNewestAttempt() {
        let rows = SendJoin.join(
            messages: [message("orig@x")],
            sends: [
                send("orig@x", status: "failed", created: 100),
                send("retry@x", status: "delivered", resentFrom: "orig@x", created: 200),
            ]
        )
        #expect(rows.count == 1)
        #expect(rows[0].status == "delivered")
    }

    @Test func newestFirst() {
        let rows = SendJoin.join(
            messages: [message("a@x", date: 100), message("b@x", date: 200)],
            sends: []
        )
        #expect(rows.map(\.date) == [200, 100])
    }
}
