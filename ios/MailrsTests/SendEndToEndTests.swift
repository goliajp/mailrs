import Foundation
import Testing

@testable import Mailrs

/// Sending, builder joined to wire.
///
/// `OutgoingMessage` is tested and `SMTPSession` is tested, and the
/// seam between them is where **a Bcc leaks**: the address belongs in
/// `RCPT TO` and nowhere in the DATA block, and only a look at what
/// actually went out can say that it is so.
@Suite(.serialized) struct SendEndToEndTests {
    private var account: MailAccount {
        var a = MailAccount.make(address: "me@example.com", displayName: "Me", sort: 0)
        a.smtpHost = "smtp.example.com"
        a.smtpPort = 465
        return a
    }

    /// A server that accepts everything, for `recipients` of them.
    ///
    /// The count is a parameter because a scripted server answers in
    /// order: one `250` too many and `DATA` reads it instead of the
    /// `354`, and the whole exchange slides by one.
    private func exchange(recipients: Int) -> [String] {
        var lines = ["220 smtp.example.com ESMTP", "250 smtp.example.com", "235 2.7.0 Accepted"]
        lines.append("250 2.1.0 sender ok")
        lines.append(contentsOf: Array(repeating: "250 2.1.5 recipient ok", count: recipients))
        lines.append(contentsOf: ["354 go ahead", "250 2.0.0 queued", "221 bye"])
        return lines
    }

    private func given(_ lines: [String]) -> ScriptedTransport {
        let script = ScriptedTransport(lines)
        AccountSender.openSmtp = { _, _ in
            SMTPSession(host: "localhost", port: 465, transport: script)
        }
        return script
    }

    private func clean() {
        AccountStore.upsert(account)
        AccountStore.saveSecret("app-password", for: account.id)
    }

    private func done() {
        AccountSender.openSmtp = { SMTPSession(host: $0, port: $1) }
        AccountStore.remove(id: account.id)
    }

    /// **The blind copy stays blind.** Its address is offered to the
    /// server as a recipient and appears nowhere in what the
    /// recipients receive — which is the whole of what "blind" means,
    /// and a mistake nobody can take back.
    @Test func aBlindCopyIsInTheEnvelopeAndNotInTheMessage() async {
        clean()
        defer { done() }
        let script = given(exchange(recipients: 3))
        let draft = OutgoingMessage.Draft(
            from: account.address, to: ["you@example.com"], cc: ["cc@example.com"],
            subject: "Lunch", body: "hello")
        let outcome = await AccountSender.send(draft, from: account, bcc: ["secret@example.com"])
        #expect(outcome == .sent)

        let sent = await script.written
        // Offered to the server, so it is delivered.
        #expect(sent.contains { $0.hasPrefix("RCPT TO:<secret@example.com>") })
        // And absent from what anybody receives.
        let data = sent.first { $0.contains("Subject:") } ?? ""
        #expect(!data.contains("secret@example.com"), "the blind copy was written into the message")
        #expect(!data.lowercased().contains("bcc:"), "a Bcc header was written")
        #expect(data.contains("Cc: cc@example.com"), "the Cc header is missing")
    }

    /// A body line of a single dot would end the DATA block. Left
    /// unstuffed, the message arrives cut in half — and the half that
    /// arrives looks like a whole message.
    @Test func aDotInTheBodySurvivesTheSend() async {
        clean()
        defer { done() }
        let script = given(exchange(recipients: 1))
        let draft = OutgoingMessage.Draft(
            from: account.address, to: ["you@example.com"], subject: "Recipe",
            body: "boil water\n.\nserve")
        _ = await AccountSender.send(draft, from: account)
        let data = await script.written.first { $0.contains("Subject:") } ?? ""
        #expect(data.contains("\r\n..\r\n"))
        #expect(data.hasSuffix("\r\n.\r\n"), "the block was never terminated")
    }

    /// The envelope sender is the account's own address. A server that
    /// permits one address will refuse another, and SPF makes that
    /// refusal correct.
    @Test func theEnvelopeSenderIsTheAccount() async {
        clean()
        defer { done() }
        let script = given(exchange(recipients: 1))
        let draft = OutgoingMessage.Draft(
            from: "someone@else.example", to: ["you@example.com"], subject: "x", body: "y")
        _ = await AccountSender.send(draft, from: account)
        #expect(await script.written.contains { $0.hasPrefix("MAIL FROM:<me@example.com>") })
    }
}
