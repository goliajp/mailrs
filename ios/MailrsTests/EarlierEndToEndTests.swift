import Foundation
import Testing

@testable import Mailrs

/// Reaching back for the mail before what is held, wire joined to
/// store.
///
/// Android has had this since "load earlier" was written; iOS had only
/// the pure `EarlierPlanTests`, so the session wiring, the mark it
/// leaves behind and the ceiling guard had never been exercised on
/// this platform at all. Writing the same tests on the second platform
/// is what found two of the three defects in this subsystem's audit.
@Suite(.serialized) struct EarlierEndToEndTests {
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
        AccountStore.replaceRows([])
        AccountStore.saveMarks([:])
        AccountStore.upsert(account)
        AccountStore.saveSecret("app-password", for: account.id)
        AccountStore.saveMarks(
            [
                "INBOX": FolderMark(
                    uidValidity: 42, highestUid: 1200, lowestUid: 1001, earlierSpan: 200)
            ], for: account.id)
    }

    private func done() {
        MailboxSyncRunner.openImap = { IMAPSession(host: $0, port: $1) }
        AccountStore.remove(id: account.id)
        AccountStore.replaceRows([])
        AccountStore.saveMarks([:])
    }

    private func header(_ subject: String) -> String {
        "From: Ada <ada@example.com>\r\nSubject: \(subject)\r\n"
            + "Date: Sun, 24 Aug 2025 01:46:40 +0000\r\nMessage-ID: <\(subject)@example.com>\r\n\r\n"
    }

    /// It asks for the span below what is held, and keeps what comes.
    @Test func itReachesBelowTheLowestHeld() async throws {
        clean()
        defer { done() }
        let body = header("older")
        let script = given([
            "* OK ready",
            "a1 OK signed in",
            "* OK [UIDVALIDITY 42] valid",
            "a2 OK selected",
            "* 1 FETCH (UID 900 FLAGS () BODY[HEADER] {\(body.utf8.count)}",
            body + ")",
            "a3 OK fetched",
        ])
        let outcome = await MailboxSyncRunner.earlier(account, folder: "INBOX")
        #expect(outcome.failure == nil, "\(outcome.failure ?? "")")
        #expect(outcome.fetched == 1)
        let written = await script.written
        let fetch = written.first { $0.contains("UID FETCH") } ?? ""
        #expect(fetch.contains("801:1000"), "\(fetch)")
        #expect(AccountStore.rows().map(\.uid) == [900])
    }

    /// **A range that is all gaps is not the end of the folder.** It
    /// returns nothing, and there may be plenty below it — so the next
    /// ask starts from the range that was tried, not from what came
    /// back, and it asks wider.
    @Test func anEmptyRangeWidensTheNextAsk() async throws {
        clean()
        defer { done() }
        _ = given([
            "* OK ready", "a1 OK signed in",
            "* OK [UIDVALIDITY 42] valid", "a2 OK selected",
            "a3 OK fetched",
        ])
        let outcome = await MailboxSyncRunner.earlier(account, folder: "INBOX")
        #expect(outcome.failure == nil)
        #expect(outcome.fetched == 0)
        let mark = AccountStore.marks(for: account.id)["INBOX"]
        #expect(mark?.lowestUid == 801, "it anchored on what came back rather than on the ask")
        #expect((mark?.earlierSpan ?? 0) > 200, "the next ask is no wider than the empty one")
    }

    /// A device already holding as much as it may is told so,
    /// **before** the network is asked.
    ///
    /// At the ceiling the cap drops the oldest rows and this fetches
    /// exactly those, so the two undo each other and the tap spends a
    /// round trip to change nothing. Refusing is the honest answer;
    /// fetching-and-discarding looks like it worked.
    ///
    /// The script is deliberately empty: reaching it at all would be
    /// the failure.
    @Test func aFullDeviceIsToldBeforeTheNetworkIs() async throws {
        clean()
        defer { done() }
        AccountStore.upsertRows(
            (1...MailboxApply.perAccount).map {
                MailboxRow(
                    accountId: account.id, uid: UInt32($0), folder: "INBOX", seen: true,
                    sender: "a@x.jp", subject: "s", date: Int64($0), messageId: "<\($0)>")
            })
        let script = given([])
        let outcome = await MailboxSyncRunner.earlier(account, folder: "INBOX")
        #expect(outcome.fetched == 0)
        #expect(
            outcome.failure?.contains("as much of this account as it can") == true,
            "it did not say the device was full: \(outcome.failure ?? "nil")")
        let written = await script.written
        #expect(written.isEmpty, "it reached the server anyway")
    }
}
