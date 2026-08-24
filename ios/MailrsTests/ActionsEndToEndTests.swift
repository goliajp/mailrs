import Foundation
import Testing

@testable import Mailrs

/// Deleting and marking unread, wire joined to store.
///
/// The rule under test is an **order**: the row goes from this device
/// only after the server says it has gone from there. A row removed
/// first and a move that then fails is a message somebody cannot see
/// and has not lost — it comes back on the next fetch, looking like a
/// bug rather than like the failure it was. Neither half alone can
/// show that.
@Suite(.serialized) struct ActionsEndToEndTests {
    private var account: MailAccount {
        var a = MailAccount.make(address: "me@example.com", displayName: "Me", sort: 0)
        a.imapHost = "imap.example.com"
        a.imapPort = 993
        return a
    }

    private var row: MailboxRow {
        MailboxRow(
            accountId: account.id, uid: 7, folder: "INBOX", seen: false,
            sender: "Ada", subject: "Lunch", date: nil, messageId: "m7")
    }

    private func given(_ lines: [String]) -> ScriptedTransport {
        let script = ScriptedTransport(lines)
        // A pool of its own, so one test's connection is not another's
        // — and so a session that stays open at the end of a test does
        // not answer the next one from the wrong script.
        MailboxActions.pool = ImapPool(open: { _, _ in IMAPSession(transport: script) })
        return script
    }

    private func clean() {
        AccountStore.upsert(account)
        AccountStore.saveSecret("app-password", for: account.id)
        AccountStore.replaceRows([row])
    }

    private func done() {
        let pool = MailboxActions.pool
        Task { await pool.dropAll() }
        MailboxActions.pool = .shared
        AccountStore.remove(id: account.id)
        AccountStore.replaceRows([])
    }

    /// The whole exchange, and the row gone afterwards.
    @Test func aDeleteMovesItAndThenForgetsIt() async {
        clean()
        defer { done() }
        let script = given([
            "* OK ready",
            "a1 OK signed in",
            #"* LIST (\HasNoChildren \Trash) "." "Deleted Items""#,
            "a2 OK listed",
            "a3 OK selected",
            "* CAPABILITY IMAP4rev1 MOVE",
            "a4 OK capabilities",
            "a5 OK moved",
        ])
        let outcome = await MailboxActions.delete(row, from: account)
        #expect(outcome == .done)
        // The name came from the server's own `\Trash` marker, not
        // from a guess — this account calls it "Deleted Items".
        let sent = await script.written
        #expect(sent.contains { $0.contains(#"UID MOVE 7 "Deleted Items""#) })
        #expect(AccountStore.rows().isEmpty, "the row survived a delete the server accepted")
    }

    /// **And the row stays when the server refuses.** This is the whole
    /// point of the order.
    @Test func aRefusedDeleteLeavesTheRowAlone() async {
        clean()
        defer { done() }
        _ = given([
            "* OK ready",
            "a1 OK signed in",
            #"* LIST (\HasNoChildren \Trash) "." "Trash""#,
            "a2 OK listed",
            "a3 OK selected",
            "* CAPABILITY IMAP4rev1 MOVE",
            "a4 OK capabilities",
            "a5 NO over quota",
        ])
        let outcome = await MailboxActions.delete(row, from: account)
        #expect(outcome != .done)
        #expect(AccountStore.rows().count == 1)
    }

    /// An account with nowhere to put it is told so, and nothing is
    /// moved — a guessed folder name has the server create one, where
    /// the message then sits invisible to every other client.
    @Test func anAccountWithNoTrashIsToldSo() async {
        clean()
        defer { done() }
        let script = given([
            "* OK ready",
            "a1 OK signed in",
            #"* LIST (\HasNoChildren) "." "INBOX""#,
            "a2 OK listed",
        ])
        let outcome = await MailboxActions.delete(row, from: account)
        #expect(outcome != .done)
        #expect(await !script.written.contains { $0.contains("MOVE") })
        #expect(AccountStore.rows().count == 1)
    }

    /// Marking unread reaches the server and this device both.
    @Test func markingUnreadTellsTheServerAndTheList() async {
        clean()
        defer { done() }
        var read = row
        read.seen = true
        AccountStore.replaceRows([read])
        let script = given(["* OK ready", "a1 OK signed in", "a2 OK selected", "a3 OK stored"])
        let outcome = await MailboxActions.markUnread(row, from: account)
        #expect(outcome == .done)
        #expect(await script.written.contains { $0.contains("UID STORE 7 -FLAGS") })
        #expect(AccountStore.rows().first?.seen == false, "the list still shows it as read")
    }
}
