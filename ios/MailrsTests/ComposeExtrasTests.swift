import Foundation
import Testing

@testable import Mailrs

struct MarkdownFormatTests {
    @Test func boldWrapsTheSelection() {
        let edit = MarkdownFormat.apply("say hi now", start: 4, end: 6, kind: .bold)
        #expect(edit == MarkdownFormat.Edit(value: "say **hi** now", start: 6, end: 8))
    }

    @Test func italicUsesUnderscoresLikeTheWeb() {
        let edit = MarkdownFormat.apply("say hi", start: 4, end: 6, kind: .italic)
        #expect(edit.value == "say _hi_")
    }

    @Test func wrappingAWrappedSelectionTakesTheMarksOff() {
        let edit = MarkdownFormat.apply("say **hi** now", start: 6, end: 8, kind: .bold)
        #expect(edit == MarkdownFormat.Edit(value: "say hi now", start: 4, end: 6))
    }

    @Test func boldWithNothingSelectedLeavesTheCaretBetweenTheMarks() {
        let edit = MarkdownFormat.apply("Hello ", start: 6, end: 6, kind: .bold)
        #expect(edit == MarkdownFormat.Edit(value: "Hello ****", start: 8, end: 8))
    }

    @Test func linkKeepsTheTextAndSelectsTheAddress() {
        let edit = MarkdownFormat.apply("see docs", start: 4, end: 8, kind: .link)
        #expect(edit.value == "see [docs](https://)")
        let picked = String(edit.value.dropFirst(edit.start).prefix(edit.end - edit.start))
        #expect(picked == "https://")
    }

    @Test func offsetsCountCharactersNotBytes() {
        let edit = MarkdownFormat.apply("件名です", start: 0, end: 2, kind: .bold)
        #expect(edit.value == "**件名**です")
    }
}

struct SendAddressesTests {
    @Test func ownAddressFirstWithoutRepeats() {
        let list = SendAddresses.list(own: "me@golia.jp", sendAs: ["sales@golia.jp", "ME@golia.jp", " "])
        #expect(list == ["me@golia.jp", "sales@golia.jp"])
    }

    @Test func aReplyLeavesByTheAddressItWasSentTo() {
        let options = ["me@golia.jp", "sales@golia.jp"]
        #expect(SendAddresses.replyFrom(options: options, recipients: "Sales <sales@golia.jp>") == "sales@golia.jp")
        #expect(SendAddresses.replyFrom(options: options, recipients: "other@example.com") == "me@golia.jp")
        #expect(SendAddresses.replyFrom(options: [], recipients: "x@y") == "")
    }

    @Test func roleReadsSendAsAndToleratesItsAbsence() throws {
        let full = try JSONDecoder().decode(Wire.Role.self, from: Data("""
            {"is_admin": false, "is_super": false, "address": "me@golia.jp", "send_as": ["sales@golia.jp"]}
            """.utf8))
        #expect(full.address == "me@golia.jp")
        #expect(full.sendAs == ["sales@golia.jp"])
        let older = try JSONDecoder().decode(Wire.Role.self, from: Data("""
            {"is_admin": true, "is_super": false}
            """.utf8))
        #expect(older.sendAs.isEmpty)
        #expect(older.isAdmin)
    }
}

struct SendHoldTests {
    @Test func everySendAsksTheServerToHoldIt() throws {
        let request = Wire.SendRequest(
            to: ["a@b.jp"], cc: [], bcc: [], subject: "s", body: "b",
            inReplyTo: nil, replyToThreadId: nil, forwardMessageId: nil,
            forwardAttachmentsFrom: nil, scheduledAt: nil
        )
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(request)) as? [String: Any]
        #expect(json?["undo_seconds"] as? Int == SendHold.seconds)
    }

    @Test func theOfferEndsBeforeTheHold() {
        #expect(SendHold.offered < .seconds(SendHold.seconds))
    }
}

struct DraftFieldsTests {
    @Test func aDraftCarriesItsAddressAndTime() throws {
        let draft = try JSONDecoder().decode(Wire.Draft.self, from: Data("""
            {"id": 3, "to": "a@b.jp", "cc": "", "bcc": "", "subject": "s", "body": "b",
             "from": "sales@golia.jp", "scheduled_at": 1900000000,
             "created_at": 1, "updated_at": 2}
            """.utf8))
        #expect(draft.from == "sales@golia.jp")
        #expect(draft.scheduledAt == 1_900_000_000)
    }

    @Test func savingADraftSendsItsAddressAndTime() throws {
        let request = Wire.SaveDraftRequest(
            id: 1, to: "a@b.jp", cc: "", bcc: "", subject: "s", body: "b",
            replyToThreadId: nil, from: "sales@golia.jp", scheduledAt: 1_900_000_000
        )
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(request)) as? [String: Any]
        #expect(json?["from"] as? String == "sales@golia.jp")
        #expect(json?["scheduled_at"] as? Int == 1_900_000_000)
    }

    @Test func plainSendKeepsAFutureTimeAndDropsAPastOne() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        #expect(SendSchedule.when(.now, kept: 1_800_000_600, now: now) == 1_800_000_600)
        #expect(SendSchedule.when(.now, kept: 1_799_999_000, now: now) == nil)
        #expect(SendSchedule.when(.now, kept: nil, now: now) == nil)
        #expect(SendSchedule.when(.laterToday, kept: 1_900_000_000, now: now) == 1_800_010_800)
    }
}

struct TemplateApplyTests {
    let thanks = Wire.Template(id: 1, name: "Thanks", subject: "Thank you", textBody: "Thank you.")
    let bodyOnly = Wire.Template(id: 2, name: "Sig", subject: "", textBody: "Best")

    @Test func aNewMessageTakesSubjectAndBody() {
        let result = TemplateApply.apply(thanks, subject: "", body: "old", isReply: false)
        #expect(result == TemplateApply.Result(subject: "Thank you", body: "Thank you."))
    }

    @Test func aReplyKeepsItsSubject() {
        let result = TemplateApply.apply(thanks, subject: "Re: x", body: "", isReply: true)
        #expect(result.subject == "Re: x")
    }

    @Test func aTemplateWithoutSubjectLeavesTheSubject() {
        let result = TemplateApply.apply(bodyOnly, subject: "Mine", body: "", isReply: false)
        #expect(result == TemplateApply.Result(subject: "Mine", body: "Best"))
    }
}

struct PushPayloadTests {
    @Test func readsTheConversationId() {
        #expect(PushPayload.threadId(["thread_id": "t1", "aps": [:]]) == "t1")
        #expect(PushPayload.threadId(["aps": [:]]) == nil)
        #expect(PushPayload.threadId(["thread_id": "  "]) == nil)
        #expect(PushPayload.threadId(["thread_id": 7]) == nil)
    }

    @Test func mapsTheTapAndBothActions() {
        let tap = PushPayload.action(for: "default", defaultIdentifier: "default", dismissIdentifier: "dismiss")
        #expect(tap == .open)
        let archive = PushPayload.action(for: PushPayload.archive, defaultIdentifier: "default", dismissIdentifier: "dismiss")
        #expect(archive == .archive)
        let read = PushPayload.action(for: PushPayload.markRead, defaultIdentifier: "default", dismissIdentifier: "dismiss")
        #expect(read == .markRead)
        let dismissed = PushPayload.action(for: "dismiss", defaultIdentifier: "default", dismissIdentifier: "dismiss")
        #expect(dismissed == .none)
    }
}
