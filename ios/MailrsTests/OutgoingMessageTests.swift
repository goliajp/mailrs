import Foundation
import Testing

@testable import Mailrs

/// Building the message that goes on the wire.
@Suite struct OutgoingMessageTests {
    private let when = Date(timeIntervalSince1970: 1_756_000_000)  // 2025-08-24 01:46:40Z
    private let tokyo = TimeZone(identifier: "Asia/Tokyo")!

    private func draft() -> OutgoingMessage.Draft {
        OutgoingMessage.Draft(from: "me@example.com", to: ["you@example.com"])
    }

    /// A numeric offset, never a zone name: `JST` and the rest are
    /// obsolete and ambiguous, and a client entitled to read them as
    /// UTC moves the message by hours.
    @Test func theDateCarriesANumericOffset() {
        let header = MailDate.rfc5322(when, timeZone: tokyo)
        #expect(header == "Sun, 24 Aug 2025 10:46:40 +0900")
        #expect(!header.contains("JST"))
    }

    /// And it must survive its own parser, which is the only check
    /// that matters.
    @Test func theDateRoundTrips() {
        for zone in ["Asia/Tokyo", "UTC", "America/Los_Angeles", "Asia/Kolkata"] {
            let header = MailDate.rfc5322(when, timeZone: TimeZone(identifier: zone)!)
            #expect(
                MailDate.epochSeconds(header) == Int64(when.timeIntervalSince1970),
                Comment(rawValue: "\(zone) produced \(header)"))
        }
    }

    /// A plain subject is left alone — encoding it makes the raw
    /// message unreadable and gains nothing.
    @Test func anAsciiSubjectIsNotEncoded() {
        #expect(EncodedWord.encode("Lunch on Thursday") == "Lunch on Thursday")
    }

    /// One that needs encoding survives its own decoder.
    @Test func anEncodedSubjectRoundTrips() {
        for subject in ["会議のお知らせ", "café ☕", "Grüße aus Köln"] {
            let encoded = EncodedWord.encode(subject)
            #expect(encoded.hasPrefix("=?utf-8?B?"))
            #expect(EncodedWord.decode(encoded) == subject, Comment(rawValue: encoded))
        }
    }

    /// A long one is folded, and each piece must still decode — the
    /// trap is splitting UTF-8 through a character, which decodes to a
    /// replacement character on every client.
    @Test func aLongEncodedSubjectFoldsWithoutBreakingACharacter() {
        let subject = String(repeating: "日本語の件名です。", count: 12)
        let encoded = EncodedWord.encode(subject)
        #expect(encoded.contains("\r\n "), "a long subject was never folded")
        for line in encoded.split(separator: "\r\n") {
            #expect(line.trimmingCharacters(in: .whitespaces).count <= 75)
        }
        #expect(EncodedWord.decode(encoded) == subject)
    }

    /// An emoji is a surrogate pair in some languages and a
    /// multi-scalar cluster in others, so a splitter that counts the
    /// wrong unit breaks it one level below where UTF-8 would.
    @Test func aRunOfEmojiNeverSplitsACharacter() {
        let subject = String(repeating: "🎌", count: 40)
        #expect(EncodedWord.decode(EncodedWord.encode(subject)) == subject)
    }

    /// A name with a comma is two recipients to a parser that reads
    /// the comma.
    @Test func aNameWithSpecialsIsQuoted() {
        #expect(
            OutgoingMessage.address("Lovelace, Ada", "a@b.com")
                == "\"Lovelace, Ada\" <a@b.com>")
        #expect(OutgoingMessage.address("Ada Lovelace", "a@b.com") == "Ada Lovelace <a@b.com>")
        #expect(OutgoingMessage.address("", "a@b.com") == "a@b.com")
    }

    /// An encoded word is already safe, and quoting one stops it being
    /// decoded at all.
    @Test func anEncodedNameIsNotQuoted() {
        let out = OutgoingMessage.address("山田 太郎", "a@b.com")
        #expect(out.hasPrefix("=?utf-8?B?"))
        #expect(!out.hasPrefix("\""))
    }

    /// Bcc lives in the envelope and nowhere else. Writing the header
    /// is how a blind copy stops being blind.
    @Test func bccIsInTheEnvelopeAndNotInTheHeaders() {
        var d = draft()
        d.cc = ["cc@example.com"]
        let recipients = OutgoingMessage.envelope(d, bcc: ["secret@example.com"])
        #expect(recipients == ["you@example.com", "cc@example.com", "secret@example.com"])

        let message = OutgoingMessage.text(d, id: "x@example.com", date: when)
        #expect(message.contains("Cc: cc@example.com"))
        #expect(!message.lowercased().contains("bcc"))
        #expect(!message.contains("secret@example.com"))
    }

    /// The same person on To and Cc is one delivery, not two.
    @Test func aDuplicateRecipientIsDeliveredOnce() {
        var d = draft()
        d.cc = ["YOU@example.com", " "]
        #expect(OutgoingMessage.envelope(d) == ["you@example.com"])
    }

    /// A reply that carries neither header starts a new conversation
    /// in every client that reads it.
    @Test func aReplyCarriesItsThreading() {
        var d = draft()
        d.inReplyTo = "<parent@example.com>"
        d.references = ["<grandparent@example.com>"]
        let message = OutgoingMessage.text(d, id: "x@example.com", date: when)
        #expect(message.contains("In-Reply-To: <parent@example.com>"))
        #expect(
            message.contains(
                "References: <grandparent@example.com> <parent@example.com>"))
    }

    /// A body with bare newlines arrives as one long line.
    @Test func everyLineEndsCRLF() {
        var d = draft()
        d.body = "one\ntwo\r\nthree\rfour"
        let message = OutgoingMessage.text(d, id: "x@example.com", date: when)
        #expect(message.hasSuffix("one\r\ntwo\r\nthree\r\nfour\r\n"))
        #expect(!message.replacingOccurrences(of: "\r\n", with: "").contains("\n"))
    }

    /// The header block ends with a blank line, and the body starts
    /// after it. Off by one here and the first line of the body is
    /// read as a header.
    @Test func theHeaderBlockIsSeparatedFromTheBody() {
        var d = draft()
        d.subject = "Hi"
        d.body = "Body starts here."
        let message = OutgoingMessage.text(d, id: "x@example.com", date: when)
        let parsed = MessageBody.extract(Data(message.utf8))
        #expect(parsed.text == "Body starts here.\r\n")
        #expect(MessageHeaders.parse(message).subject == "Hi")
        #expect(MessageHeaders.parse(message).messageId == "<x@example.com>")
    }

    /// A message this builder makes must be readable by the reader on
    /// the other side of this same app.
    @Test func whatIsBuiltIsWhatIsRead() {
        var d = draft()
        d.fromName = "山田 太郎"
        d.subject = "会議のお知らせ"
        d.body = "本文です。\n二行目。"
        let message = OutgoingMessage.text(d, id: "x@example.com", date: when, timeZone: tokyo)
        let headers = MessageHeaders.parse(message)
        #expect(headers.subject == "会議のお知らせ")
        #expect(headers.from.contains("山田 太郎"))
        #expect(MailDate.epochSeconds(headers.date) == Int64(when.timeIntervalSince1970))
        #expect(MessageReader.display(of: Data(message.utf8)).text == "本文です。\r\n二行目。\r\n")
    }
}
