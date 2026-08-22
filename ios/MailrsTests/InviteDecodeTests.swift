import Foundation
import Testing

@testable import Mailrs

/// The stub's own answer, decoded.
///
/// The card renders nothing when `invite_payload` fails to decode, and
/// it fails **silently** — `try?` in `MessageDetail.init` turns a
/// throw into a nil invitation, which looks exactly like a message that
/// carries none. On iOS the card was blank while Android rendered the
/// same bytes, and no assertion said so. This one does: it decodes the
/// bytes the stub actually sends, with `try` rather than `try?`, so a
/// shape it cannot read is an error rather than an empty card.
struct InviteDecodeTests {
    // Raw delimiters: the payload contains `\n` inside a JSON string,
    // and an ordinary Swift literal turns that into a real newline —
    // which `JSONSerialization` rejects as an unescaped control
    // character while `JSONDecoder` quietly accepts it. The fixture has
    // to be the bytes the server sends, not a near-miss.
    static let response = #"""
{
  "uid": 2,
  "sender": "chair@example.com",
  "recipients": "me@golia.jp",
  "subject": "Product sync",
  "internal_date": 1754400000,
  "message_id": "<m2@x>",
  "text_body": "See you then.",
  "html_body": "",
  "flags": 0,
  "invite_method": "REQUEST",
  "invite_payload": {
    "uid": "040000008200E00074C5B7101A82E00800000000EXAMPLE",
    "sequence": 9,
    "summary": "Product sync",
    "location": "SCL.H-120 (11) Teams Room (Santa Clara)",
    "description": "Microsoft Teams meeting\nJoin: https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc/0",
    "method": "REQUEST",
    "organizer": {
      "cn": "Chris Dai",
      "email": "chair@example.com"
    },
    "attendees": [
      {
        "cn": "Me",
        "email": "me@golia.jp",
        "partstat": "NEEDS-ACTION",
        "role": "REQ-PARTICIPANT",
        "rsvp": true
      },
      {
        "cn": "Shu Wang",
        "email": "shu@example.com",
        "partstat": "ACCEPTED",
        "role": "REQ-PARTICIPANT",
        "rsvp": true
      },
      {
        "cn": "Minhao Jin",
        "email": "minhao@example.com",
        "partstat": "NEEDS-ACTION",
        "role": "REQ-PARTICIPANT",
        "rsvp": true
      }
    ],
    "status": "CONFIRMED",
    "dtstart": {
      "Zoned": {
        "local": "2026-08-20T16:00:00",
        "tz_name": "Pacific Standard Time"
      }
    },
    "dtend": {
      "Zoned": {
        "local": "2026-08-20T16:50:00",
        "tz_name": "Pacific Standard Time"
      }
    },
    "join_url": "https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc/0",
    "dtstart_utc": "2026-08-20T23:00:00+00:00",
    "dtend_utc": "2026-08-20T23:50:00+00:00",
    "rrule": null,
    "recurrence_id": null
  },
  "rsvp_status": null,
  "rsvp_at": null
}
"""#

    @Test func theServersOwnAnswerDecodes() throws {
        let detail = try JSONDecoder().decode(
            Wire.MessageDetail.self, from: Data(Self.response.utf8))
        #expect(detail.inviteMethod == "REQUEST")
        let invite = try #require(detail.invite, "the invitation did not decode")
        #expect(invite.summary == "Product sync")
        #expect(invite.sequence == 9)
        #expect(invite.attendees.count == 3)
        #expect(invite.organiserZone == "Pacific Standard Time")
        #expect(invite.joinURL?.host() == "teams.microsoft.com")
        // 16:00 in Santa Clara on 20 August is 23:00 UTC.
        #expect(invite.startsAt?.timeIntervalSince1970 == 1_787_266_800)
    }

    /// And the invitation decodes on its own, so a failure points at
    /// the payload rather than at the envelope around it.
    @Test func thePayloadDecodesOnItsOwn() throws {
        let whole = try #require(
            try JSONSerialization.jsonObject(with: Data(Self.response.utf8)) as? [String: Any])
        let payload = try #require(whole["invite_payload"])
        let bytes = try JSONSerialization.data(withJSONObject: payload)
        let invite = try JSONDecoder().decode(Wire.Invite.self, from: bytes)
        #expect(invite.uid.hasPrefix("040000008200E000"))
    }

    /// And the thread response, which is what decides whether the card
    /// is mounted at all.
    ///
    /// The card is blank on screen when this field is empty, and empty
    /// is also what "no calendar part" looks like — so the screen
    /// cannot tell them apart and neither could I. This can.
    @Test func theThreadResponseSaysWhichMessageIsAnInvitation() throws {
        let rows = try JSONDecoder().decode([Wire.Message].self, from: Data(Self.thread.utf8))
        #expect(rows.count == 2)
        #expect(rows[0].inviteMethod.isEmpty, "the first message carries no calendar part")
        #expect(rows[1].inviteMethod == "REQUEST", "the second one does, and the card reads this")
    }

    static let thread = #"""
[
  {
    "uid": 1,
    "sender": "Alice Smith <alice@example.com>",
    "sender_trust": "verified",
    "recipients": "me@golia.jp",
    "subject": "Quarterly report",
    "flags": 0,
    "internal_date": 1754400000,
    "message_id": "<m1@x>",
    "text_body": "plain fallback",
    "html_body": "<table width=\"760\" style=\"width:760px\"><tr><td><div style=\"width:760px;background:#eef;padding:8px\"><img src=\"https://tracker.example.com/open.gif\" width=\"1\" height=\"1\"><h1>Newsletter</h1><p>lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet lorem ipsum dolor sit amet </p></div></td></tr></table>",
    "attachments": [
      {
        "filename": "請求書_2026年8月分.pdf",
        "content_type": "application/pdf",
        "size": 1234
      },
      {
        "filename": "logo.png",
        "content_type": "image/png",
        "size": 70
      }
    ],
    "category": "inbox",
    "risk_score": 0,
    "risk_reason": "",
    "summary": "",
    "people": {},
    "dates": {},
    "amounts": {},
    "action_items": [],
    "ai_analyzed": false,
    "importance_level": "normal",
    "importance_score": 0.1,
    "is_bulk_sender": false,
    "has_tracking_pixel": false,
    "requires_action": false,
    "sender_intent": "",
    "invite_method": ""
  },
  {
    "uid": 2,
    "sender": "spoofed@example.com",
    "sender_trust": "suspicious",
    "recipients": "me@golia.jp, Bob <bob@example.com>",
    "subject": "Quarterly report",
    "flags": 0,
    "internal_date": 1754400000,
    "message_id": "<m2@x>",
    "text_body": "plain fallback",
    "html_body": "<p>Short reply, narrow body.</p>",
    "attachments": [],
    "category": "inbox",
    "risk_score": 0,
    "risk_reason": "",
    "summary": "",
    "people": {},
    "dates": {},
    "amounts": {},
    "action_items": [],
    "ai_analyzed": false,
    "importance_level": "normal",
    "importance_score": 0.1,
    "is_bulk_sender": false,
    "has_tracking_pixel": false,
    "requires_action": false,
    "sender_intent": "",
    "invite_method": "REQUEST"
  }
]
"""#
}
