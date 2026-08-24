import Foundation
import Testing

@testable import Mailrs

/// The one thing no other test in this repo does: reach a mail server
/// over a **real socket, with real TLS, and a certificate the app
/// actually validates**.
///
/// Every other IMAP and SMTP assertion here is made against a scripted
/// transport — a fake that hands the session lines from a list. That
/// covers the conversation and nothing under it: the handshake, the
/// certificate check, the socket's own framing, and
/// `UpgradableTransport`, the `Stream` implementation STARTTLS needs,
/// which had never been run at all on this platform. It was the thing
/// I was least sure of, and the thing a scripted transport can never
/// reach.
///
/// The app is not modified for this. It validates the certificate as
/// it always does; the simulator has been given a root to trust —
/// `scripts/ios-build.sh` generates it per run and installs it with
/// `simctl keychain add-root-cert`. A release build on a real phone
/// trusts nothing extra, which is the point of doing it this way
/// rather than by relaxing the client.
///
/// Skipped when the stub is not listening, so running the suite from
/// Xcode without the script does not report a failure about a server
/// that was never started — an absent measuring device must not look
/// like data.
@Suite(.serialized) struct RealTLSMailTests {
    private static let host = "127.0.0.1"
    private static let imaps: UInt16 = 9993
    private static let submission: UInt16 = 9587
    /// Serves a certificate signed by an authority installed nowhere.
    private static let untrusted: UInt16 = 9994

    private func stubIsUp(_ port: UInt16) -> Bool {
        let sock = socket(AF_INET, SOCK_STREAM, 0)
        guard sock >= 0 else { return false }
        defer { close(sock) }
        var address = sockaddr_in()
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        address.sin_addr.s_addr = inet_addr(RealTLSMailTests.host)
        let ok = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(sock, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) == 0
            }
        }
        return ok
    }

    private var account: MailAccount {
        var a = MailAccount.make(address: "me@example.com", displayName: "Me", sort: 0)
        a.imapHost = RealTLSMailTests.host
        a.imapPort = RealTLSMailTests.imaps
        a.smtpHost = RealTLSMailTests.host
        a.smtpPort = RealTLSMailTests.submission
        return a
    }

    /// A whole pass: TLS to a real listener, sign in, list, select,
    /// fetch, and a row in the store a list could show.
    @Test func aPassOverRealTlsFillsTheStore() async throws {
        try #require(stubIsUp(RealTLSMailTests.imaps), "the TLS mail stub is not listening")
        AccountStore.replaceRows([])
        AccountStore.saveMarks([:])
        AccountStore.upsert(account)
        AccountStore.saveSecret("app-password", for: account.id)
        defer {
            AccountStore.remove(id: account.id)
            AccountStore.replaceRows([])
            AccountStore.saveMarks([:])
        }

        let outcome = await MailboxSyncRunner.run(account)
        #expect(outcome.failure == nil, "\(outcome.failure ?? "")")
        #expect(outcome.fetched == 2)

        let rows = AccountStore.rows().sorted { $0.uid < $1.uid }
        #expect(rows.map(\.uid) == [1001, 1002])
        // Decoded on the way through, which is what makes this a test
        // of the chain rather than of the socket: the subject crossed
        // the wire as an RFC 2047 encoded word.
        #expect(rows.first?.subject == "会議")
        #expect(rows.first?.sender == "Ada")
    }

    /// **The path with no coverage at all.** Plaintext submission,
    /// `STARTTLS`, and the connection upgraded in place — on iOS that
    /// is `UpgradableTransport`, which `NWConnection` could not do and
    /// which nothing had ever run.
    ///
    /// The stub refuses `AUTH` before the upgrade, so a client that
    /// skipped it would fail here rather than quietly send a password
    /// in the clear.
    @Test func aMessageGoesOutThroughStarttls() async throws {
        try #require(
            stubIsUp(RealTLSMailTests.submission), "the TLS mail stub is not listening")
        AccountStore.upsert(account)
        AccountStore.saveSecret("app-password", for: account.id)
        defer { AccountStore.remove(id: account.id) }

        let draft = OutgoingMessage.Draft(
            from: account.address, to: ["you@example.com"],
            subject: "会議のご案内", body: "本文です。\n.\n終わり")
        let outcome = await AccountSender.send(draft, from: account)
        #expect(outcome == .sent, "\(outcome)")
    }

    /// **A certificate this device does not trust is an error, not a
    /// wait.**
    ///
    /// It was a wait. `NWConnection` reports a handshake the peer
    /// refuses as `.waiting`, not `.failed`, and then sits in it
    /// indefinitely; `TLSTransport.connect` handled `.failed` and
    /// `.cancelled` and let `.waiting` fall through to `default: break`.
    /// So a server with an expired certificate, or a proxy in the
    /// middle, gave an app that **hung** — no error, no timeout, for
    /// exactly the people whose network is being interfered with.
    ///
    /// No scripted transport has a handshake to refuse, which is why
    /// this needed a real socket and a certificate nobody trusts. The
    /// stub serves one on its own port for this test alone.
    @Test func anUntrustedCertificateFailsRatherThanHangs() async throws {
        try #require(stubIsUp(RealTLSMailTests.untrusted), "the TLS mail stub is not listening")
        var rogue = account
        rogue.imapPort = RealTLSMailTests.untrusted
        AccountStore.upsert(rogue)
        AccountStore.saveSecret("app-password", for: rogue.id)
        defer { AccountStore.remove(id: rogue.id) }

        let began = Date()
        let outcome = await MailboxSyncRunner.run(rogue)
        let took = Date().timeIntervalSince(began)

        #expect(outcome.failure != nil, "an untrusted certificate was accepted")
        // Well inside the 20-second connect timeout: this must be the
        // handshake being reported, not the deadline expiring. A test
        // that only checks "it eventually failed" would pass on the
        // defect this was written for.
        #expect(took < 10, "it took \(took)s — that is the timeout, not a refusal")
    }
}
