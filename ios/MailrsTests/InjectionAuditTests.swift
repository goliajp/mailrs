import Foundation
import Testing

@testable import Mailrs

/// Does a header from a stranger reach a command line?
///
/// An audit, written to find out rather than to confirm. A reply's
/// recipient comes from the `Reply-To:` of a message somebody else
/// wrote, and an encoded word decodes to **anything at all** — so if
/// a CRLF survives that trip, replying to a hostile message injects
/// SMTP commands into this client's session.
///
/// All four failed when first written. They are the audit's finding,
/// kept as its guard.
@Suite struct InjectionAuditTests {
    private func encoded(_ text: String) -> String {
        "=?utf-8?B?" + Data(text.utf8).base64EncodedString() + "?="
    }

    /// What a decoded `Reply-To` actually contains.
    @Test func aDecodedReplyToCannotCarryALineBreak() {
        let nasty = encoded("victim@example.com>\r\nRCPT TO:<attacker@evil.example")
        let headers = MessageHeaders.parse("Reply-To: \(nasty)\r\nFrom: a@b\r\n\r\nbody")
        let recipient = ReplyDraft.recipient(headers)
        #expect(
            !recipient.contains("\r") && !recipient.contains("\n"),
            Comment(rawValue: "a line break reached the recipient: <\(recipient)>"))
    }

    /// And the same value as it would reach a `Subject:` header.
    @Test func aDecodedSubjectCannotCarryALineBreak() {
        let nasty = encoded("Hi\r\nBcc: attacker@evil.example")
        let headers = MessageHeaders.parse("Subject: \(nasty)\r\nFrom: a@b\r\n\r\nbody")
        #expect(
            !headers.subject.contains("\r") && !headers.subject.contains("\n"),
            Comment(rawValue: "a line break reached the subject: <\(headers.subject)>"))
    }

    /// A built reply must not gain a header from either.
    @Test func aBuiltReplyHasNoInjectedHeader() {
        let nasty = encoded("victim@example.com>\r\nBcc: attacker@evil.example\r\nX: <")
        let headers = MessageHeaders.parse("Reply-To: \(nasty)\r\nFrom: a@b\r\n\r\nbody")
        let me = MailAccount.make(address: "me@example.com", displayName: "Me", sort: 0)
        let draft = ReplyDraft.make(to: headers, from: me)
        let message = OutgoingMessage.text(
            draft, id: "x@example.com",
            date: Date(timeIntervalSince1970: 1_756_000_000),
            timeZone: TimeZone(identifier: "UTC")!)
        let injected = message.components(separatedBy: "\r\n").contains {
            $0.lowercased().hasPrefix("bcc:") || $0.hasPrefix("X:")
        }
        #expect(!injected, "a header was injected through Reply-To")
    }

    /// **The envelope is a command line.** An address with a control
    /// character in it does not become a worse address — it becomes
    /// another SMTP command, and the message goes somewhere the sender
    /// never typed.
    @Test func anAddressWithALineBreakNeverReachesTheEnvelope() {
        var draft = OutgoingMessage.Draft(
            from: "me@example.com",
            to: ["you@example.com", "victim@x.example>\r\nRCPT TO:<attacker@evil.example"])
        draft.cc = ["ok@example.com"]
        let envelope = OutgoingMessage.envelope(draft, bcc: ["bcc\r\nDATA@x.example"])
        for address in envelope {
            #expect(
                !address.unicodeScalars.contains { $0.value < 0x20 || $0.value == 0x7F },
                Comment(rawValue: "a control character reached the envelope: <\(address)>"))
        }
        // And the good ones are still there — a rule that drops
        // everything would pass the assertion above and send nothing.
        #expect(envelope.contains("you@example.com"))
        #expect(envelope.contains("ok@example.com"))
    }
}
