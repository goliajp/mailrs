import Foundation
import Testing

@testable import Mailrs

/// The wire joined to the store.
///
/// Every rule above the socket is asserted somewhere, and every socket
/// conversation is asserted against a scripted transport — and until
/// this existed the two had never been checked **together**. A pass
/// that talks to the server correctly and files the answer in the
/// wrong place passes both halves and shows nobody their mail.
///
/// The store is `UserDefaults`, which a test process has to itself, so
/// unlike Android this needs no device: only the credential lives in
/// the keychain, and the simulator has one.
@Suite(.serialized) struct SyncEndToEndTests {
    private var account: MailAccount {
        var a = MailAccount.make(address: "me@example.com", displayName: "Me", sort: 0)
        a.imapHost = "imap.example.com"
        a.imapPort = 993
        return a
    }

    private func given(_ lines: [String]) -> ScriptedTransport {
        let script = ScriptedTransport(lines)
        MailboxSyncRunner.openImap = { _, _ in IMAPSession(transport: script) }
        return script
    }

    private func clean() {
        AccountStore.saveRows([])
        AccountStore.saveMarks([:])
        AccountStore.upsert(account)
        AccountStore.saveSecret("app-password", for: account.id)
    }

    private func done() {
        MailboxSyncRunner.openImap = { IMAPSession(host: $0, port: $1) }
        AccountStore.remove(id: account.id)
        AccountStore.saveRows([])
        AccountStore.saveMarks([:])
    }

    private func header(_ subject: String) -> String {
        "From: Ada <ada@example.com>\r\nSubject: \(subject)\r\n"
            + "Date: Sun, 24 Aug 2025 01:46:40 +0000\r\nMessage-ID: <m7@example.com>\r\n\r\n"
    }

    /// One pass, from the greeting to a row a list could show. The
    /// subject arrives encoded, because that is how a non-ASCII one
    /// always does.
    @Test func aPassPutsAReadableRowInTheStore() async throws {
        clean()
        defer { done() }
        let body = header("=?utf-8?B?5Lya6K2w?=")
        _ = given([
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in",
            #"* LIST (\HasNoChildren) "." "INBOX""#,
            "a2 OK listed",
            "* 1 EXISTS",
            "* OK [UIDVALIDITY 42] valid",
            "a3 OK selected",
            "* 1 FETCH (UID 7 FLAGS () BODY[HEADER] {\(body.utf8.count)}",
            body + ")",
            "a4 OK fetched",
        ])
        let outcome = await MailboxSyncRunner.run(account)
        #expect(outcome.failure == nil, Comment(rawValue: outcome.failure ?? ""))
        #expect(outcome.fetched == 1)

        let rows = AccountStore.rows()
        #expect(rows.count == 1)
        // Decoded, not `=?utf-8?B?…?=` — a row shows what somebody
        // wrote, and the decoding happens far from here.
        #expect(rows.first?.subject == "会議")
        #expect(rows.first?.seen == false)
        #expect(rows.first?.folder == "INBOX")
        #expect(rows.first?.uid == 7)
        #expect(rows.first?.accountId == account.id)

        // And the place is remembered, or the next pass fetches it all
        // over again.
        #expect(AccountStore.marks(for: account.id)["INBOX"]?.uidValidity == 42)
        #expect(AccountStore.marks(for: account.id)["INBOX"]?.highestUid == 7)
    }

    /// The second pass asks only for what is new — the whole point of
    /// remembering a place — and applies what it learns about the one
    /// already here.
    @Test func aSecondPassAsksOnlyForWhatIsNew() async throws {
        clean()
        defer { done() }
        AccountStore.saveMarks(["INBOX": FolderMark(uidValidity: 42, highestUid: 7)],
                               for: account.id)
        AccountStore.saveRows([
            MailboxRow(
                accountId: account.id, uid: 7, folder: "INBOX", seen: false,
                sender: "Ada", subject: "old", date: nil, messageId: "m7")
        ])
        let script = given([
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in",
            #"* LIST (\HasNoChildren) "." "INBOX""#,
            "a2 OK listed",
            "* OK [UIDVALIDITY 42] valid",
            "a3 OK selected",
            "a4 OK nothing new",
            #"* 1 FETCH (UID 7 FLAGS (\Seen))"#,
            "a5 OK flags",
        ])
        _ = await MailboxSyncRunner.run(account)

        let sent = await script.written
        let fetch = sent.first { $0.contains("UID FETCH") && $0.contains("BODY.PEEK") } ?? ""
        #expect(fetch.contains("8:*"), Comment(rawValue: fetch))

        // A message read on a laptop stops being bold here.
        #expect(AccountStore.rows().first?.seen == true)
    }

    /// A server that refuses the credential must leave the store alone
    /// and say why — an account that quietly fetches nothing is
    /// indistinguishable from an account with no new mail.
    @Test func aRefusedSignInSaysSoAndChangesNothing() async throws {
        clean()
        defer { done() }
        _ = given([
            "* OK ready",
            "a1 NO [AUTHENTICATIONFAILED] Invalid credentials",
        ])
        let outcome = await MailboxSyncRunner.run(account)
        #expect(outcome.failure != nil)
        #expect(AccountStore.rows().isEmpty)
        #expect(AccountStore.marks(for: account.id).isEmpty)
    }
}
