import Foundation
import Testing

@testable import Mailrs

/// A scripted server that can also be made to behave as one that hung
/// up while nothing was happening.
actor DroppableTransport: ByteTransport {
    private var lines: [String]
    private(set) var written: [String] = []
    private(set) var closes = 0
    private var dead = false

    init(_ lines: [String]) { self.lines = lines }

    /// Behave from now on as a connection the server has dropped.
    func hangUp() { dead = true }

    func connect() async throws {}

    func receive() async throws -> Data {
        if dead { throw TransportFailure.closed }
        guard !lines.isEmpty else { throw TransportFailure.closed }
        return Data((lines.removeFirst() + "\r\n").utf8)
    }

    func send(_ data: Data) async throws {
        if dead { throw TransportFailure.closed }
        written.append(String(decoding: data, as: UTF8.self))
    }

    func upgradeToTLS() async throws {}
    func close() { closes += 1 }
}

/// Keeping one IMAP connection per account.
///
/// The interesting assertion is not that a second action skips the
/// handshake — that one is easy and would pass on a pool with the
/// defect. It is that a connection the server has since dropped does
/// not surface as a failed action, because that is what a naive pool
/// ships and what nobody sees while they are looking.
@Suite struct ImapPoolTests {
    private var account: MailAccount {
        var a = MailAccount.make(address: "me@x.jp", displayName: "Me", sort: 0)
        a.imapHost = "imap.x.jp"
        a.imapPort = 993
        return a
    }

    /// Greeting, `LOGIN` accepted, then whatever else is asked for.
    private func script(_ extra: [String] = []) -> DroppableTransport {
        DroppableTransport(["* OK ready", "a1 OK logged in"] + extra)
    }

    private func pool(
        _ scripts: [DroppableTransport],
        clock: @escaping @Sendable () -> TimeInterval = { 0 }
    ) -> (ImapPool, Handed) {
        let handed = Handed(scripts)
        let pool = ImapPool(
            open: { _, _ in IMAPSession(transport: handed.next()) },
            now: clock)
        return (pool, handed)
    }

    /// The scripts waiting to be handed out, and how many were.
    ///
    /// A class with a lock rather than captured variables: the pool
    /// hands connections out from whatever task asked for one, so this
    /// is genuinely shared and Swift 6 is right to say so.
    final class Handed: @unchecked Sendable {
        private var queue: [DroppableTransport]
        private var opened: [DroppableTransport] = []
        private let lock = NSLock()

        init(_ scripts: [DroppableTransport]) { queue = scripts }

        func next() -> DroppableTransport {
            lock.lock()
            defer { lock.unlock() }
            let script = queue.removeFirst()
            opened.append(script)
            return script
        }

        var count: Int {
            lock.lock()
            defer { lock.unlock() }
            return opened.count
        }
    }

    // Two actions in a row are one connection: the second pays no TLS
    // handshake and no LOGIN, which is the whole point.
    @Test func aSecondActionReusesTheConnection() async throws {
        let first = script(["a2 OK done", "a3 OK done"])
        let (pool, handed) = pool([first, script()])
        _ = try await pool.use(account, secret: "pw") { try await $0.capabilities() }
        _ = try await pool.use(account, secret: "pw") { try await $0.capabilities() }
        #expect(handed.count == 1, "a second connection was opened")
        let logins = await first.written.filter { $0.contains("LOGIN") }.count
        #expect(logins == 1, "logged in twice")
    }

    // **The one that matters.** A connection kept past the freshness
    // window and dropped by the server must be replaced, not reported.
    // Nothing about this is visible in the moment it is created — it
    // only appears after a pause, which is to say when nobody is
    // watching.
    @Test func aDroppedConnectionIsReplacedNotReported() async throws {
        let stale = script()
        let fresh = script(["a2 OK done"])
        let clock = Clock()
        let (pool, handed) = pool([stale, fresh], clock: { clock.value })
        _ = try await pool.use(account, secret: "pw") { _ in }
        // Time passes, and the server hangs up while nothing is happening.
        clock.value = ImapPool.freshWindow + 1
        await stale.hangUp()
        let answered = try await pool.use(account, secret: "pw") { session -> String in
            _ = try await session.capabilities()
            return "done"
        }
        #expect(answered == "done", "the action failed on a server that was fine")
        #expect(handed.count == 2, "the dead connection was not replaced")
        let closes = await stale.closes
        #expect(closes > 0, "the dead socket was left open")
    }

    // Inside the window the probe is skipped — a person filing a run of
    // messages should not pay a round trip between each.
    @Test func aConnectionUsedMomentsAgoIsNotProbed() async throws {
        let only = script(["a2 OK done"])
        let (pool, _) = pool([only, script()])
        _ = try await pool.use(account, secret: "pw") { _ in }
        _ = try await pool.use(account, secret: "pw") { _ in }
        let probes = await only.written.filter { $0.contains("NOOP") }.count
        #expect(probes == 0, "a NOOP was sent inside the freshness window")
    }

    @Test func beyondTheWindowTheConnectionIsProbed() async throws {
        let only = script(["a2 OK noop done"])
        let clock = Clock()
        let (pool, _) = pool([only, script()], clock: { clock.value })
        _ = try await pool.use(account, secret: "pw") { _ in }
        clock.value = ImapPool.freshWindow + 1
        _ = try await pool.use(account, secret: "pw") { _ in }
        let probes = await only.written.filter { $0.contains("NOOP") }.count
        #expect(probes == 1, "the connection was handed out unasked")
    }

    // A session that threw may have an answer still in flight, and the
    // next caller would read it as its own.
    @Test func aFailedSessionIsNotHandedOutAgain() async throws {
        let broken = script()
        let (pool, handed) = pool([broken, script(["a2 OK done"])])
        _ = try await pool.use(account, secret: "pw") { _ in }
        await broken.hangUp()
        _ = try? await pool.use(account, secret: "pw") { try await $0.capabilities() }
        _ = try await pool.use(account, secret: "pw") { _ in }
        #expect(handed.count == 2, "the failed session was handed out again")
        let kept = await pool.count()
        #expect(kept == 1, "more than one connection survived")
        let closes = await broken.closes
        #expect(closes > 0, "the failed socket was left open")
    }

    // A credential that has just been deleted must not leave a socket
    // open that is still signed in with it.
    @Test func droppingAnAccountClosesItsConnection() async throws {
        let only = script()
        let (pool, _) = pool([only, script()])
        _ = try await pool.use(account, secret: "pw") { _ in }
        await pool.drop(account.id)
        let closes = await only.closes
        #expect(closes > 0, "the socket was left open")
        let kept = await pool.count()
        #expect(kept == 0)
    }

    // Two accounts are two connections: a server caps them per account,
    // and one shared socket would be signed in as the wrong person.
    @Test func eachAccountGetsItsOwnConnection() async throws {
        let (pool, handed) = pool([script(), script()])
        // A different **id**, not just a different address: the pool
        // is keyed on the id, and an account is identified by it
        // everywhere else too. Changing only the address made this
        // test pass a single connection off as two.
        var other = MailAccount.make(address: "you@x.jp", displayName: "You", sort: 1)
        other.imapHost = "imap.x.jp"
        other.imapPort = 993
        #expect(other.id != account.id, "the two accounts are the same account")
        _ = try await pool.use(account, secret: "pw") { _ in }
        _ = try await pool.use(other, secret: "pw") { _ in }
        #expect(handed.count == 2)
        let kept = await pool.count()
        #expect(kept == 2)
    }

    /// A clock the test moves by hand — `Date()` cannot be asked to be
    /// thirty seconds later.
    final class Clock: @unchecked Sendable {
        var value: TimeInterval = 0
    }

    // An account removed **while an action is running**. The first
    // version of this pool took the connection out of the table for the
    // length of the call, so `drop` found nothing to close and the
    // action then put it back — leaving a socket open and signed in
    // with a credential that had just been deleted.
    @Test func anAccountDroppedMidActionDoesNotComeBack() async throws {
        let only = script(["a2 OK done"])
        let (pool, _) = pool([only, script()])
        _ = try await pool.use(account, secret: "pw") { session in
            await pool.drop(self.account.id)
            _ = try await session.capabilities()
        }
        let kept = await pool.count()
        #expect(kept == 0, "the dropped connection was put back")
        let closes = await only.closes
        #expect(closes > 0, "the dropped socket was left open")
    }
}
