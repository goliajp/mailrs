import Foundation
import Testing

@testable import Mailrs

/// Deleting from a POP3 mailbox.
///
/// Three things that are not true of IMAP, and each is a way to lose
/// or fail to lose a message quietly.
@Suite(.serialized) struct Pop3DeleteEndToEndTests {
    private let uidl = "QhdPYR-a"

    private var account: MailAccount {
        var a = MailAccount.make(address: "me@example.com", displayName: "Me", sort: 0)
        a.imapHost = "pop.example.com"
        a.imapPort = 995
        a.incoming = .pop3
        return a
    }

    private var row: MailboxRow {
        MailboxRow(
            accountId: account.id, uid: MailboxSyncRunner.foldedUid(uidl), folder: "INBOX",
            seen: false, sender: "Ada", subject: "Lunch", date: nil, messageId: "m1")
    }

    private func given(_ lines: [String]) -> ScriptedTransport {
        let script = ScriptedTransport(lines)
        MailboxActions.openPop3 = { _, _ in POP3Session(transport: script) }
        return script
    }

    private func clean() {
        AccountStore.upsert(account)
        AccountStore.saveSecret("app-password", for: account.id)
        AccountStore.saveRows([row])
        AccountStore.savePopSeen(account.id, [uidl])
    }

    private func done() {
        MailboxActions.openPop3 = { POP3Session(host: $0, port: $1) }
        AccountStore.remove(id: account.id)
        AccountStore.saveRows([])
    }

    /// **The number is only valid in this session**, so the uidl is
    /// looked up now — a stored number would delete whatever happens
    /// to be in that position today. Here the message has moved from 1
    /// to 3 since it was fetched.
    ///
    /// And **`DELE` does not delete**: the server acts at `QUIT`, so a
    /// session dropped after `DELE` leaves the mailbox untouched.
    @Test func theNumberIsLookedUpNowAndQuitCommitsIt() async {
        clean()
        defer { done() }
        let script = given([
            "+OK POP3 ready",
            "+OK user accepted",
            "+OK signed in",
            "+OK listing",
            "1 QhdPYR-other",
            "2 QhdPYR-another",
            "3 \(uidl)",
            ".",
            "+OK marked",
            "+OK bye",
        ])
        let outcome = await MailboxActions.delete(row, from: account)
        #expect(outcome == .done)
        let sent = await script.written
        #expect(sent.contains { $0.hasPrefix("DELE 3") })
        #expect(sent.contains { $0.hasPrefix("QUIT") }, "DELE without QUIT deletes nothing")
        #expect(AccountStore.rows().isEmpty)
    }

    /// **A message already gone is a success.** It was deleted from
    /// another device, and telling somebody their delete failed when
    /// the thing is gone is a lie that makes them try again.
    @Test func aMessageAlreadyGoneIsNotAnError() async {
        clean()
        defer { done() }
        let script = given([
            "+OK POP3 ready",
            "+OK user accepted",
            "+OK signed in",
            "+OK listing",
            "1 QhdPYR-other",
            ".",
            "+OK bye",
        ])
        let outcome = await MailboxActions.delete(row, from: account)
        #expect(outcome == .done)
        // Nothing was marked, because there was nothing to mark.
        #expect(await !script.written.contains { $0.hasPrefix("DELE") })
        #expect(AccountStore.rows().isEmpty)
    }

    /// A refused sign-in leaves the row alone and says why.
    @Test func aRefusedSignInLeavesTheRow() async {
        clean()
        defer { done() }
        _ = given(["+OK POP3 ready", "+OK user accepted", "-ERR [AUTH] Invalid password"])
        let outcome = await MailboxActions.delete(row, from: account)
        #expect(outcome != .done)
        #expect(AccountStore.rows().count == 1)
    }
}
