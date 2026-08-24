import Foundation

/// One live IMAP connection per account, reused between actions.
///
/// Every tap used to be its own connection: TCP, TLS, `LOGIN`, the one
/// command it came for, `LOGOUT`. On a phone that is a second or two
/// before anything happens, for a command that takes milliseconds —
/// and a person filing ten messages paid it ten times.
///
/// **One connection, not a pool of several.** IMAP servers cap
/// concurrent connections per account (Gmail allows fifteen, and counts
/// every device), and a phone is doing one thing at a time. A second
/// connection would buy nothing and spend somebody's quota.
///
/// ### The part that is not about speed
///
/// A kept connection may already be dead. Servers drop idle ones, NATs
/// drop them sooner, and a socket gives no sign of it until something
/// is written. A pool that does not account for this ships a specific
/// defect: **the first tap after a while fails, and says the server
/// could not be reached about a server that is fine.** Worse, it is
/// intermittent by construction — it happens after a pause, so it
/// never happens while anybody is looking.
///
/// So a connection older than ``freshWindow`` is asked `NOOP` before it
/// is handed out, and one that does not answer is replaced.
///
/// **The action itself is not retried.** A failure part-way through a
/// `MOVE` cannot be told from a failure before it, and repeating it
/// would file the message twice. The probe is what makes retrying
/// unnecessary in the common case; where it is not enough, failing is
/// the honest answer.
actor ImapPool {
    /// How recently a connection must have been used to be handed out
    /// without asking the server whether it is still there.
    ///
    /// Short, because the cost of being wrong is a failed action and
    /// the cost of being careful is one round trip. Somebody filing a
    /// run of messages stays inside it; somebody coming back to the app
    /// does not, and pays a `NOOP` instead of a lie.
    static let freshWindow: TimeInterval = 30

    /// The one pool for this process.
    static let shared = ImapPool()

    private struct Held {
        let session: IMAPSession
        var lastUsed: TimeInterval
    }

    private let open: @Sendable (String, UInt16) -> IMAPSession
    private let now: @Sendable () -> TimeInterval
    private var held: [String: Held] = [:]

    /// Which accounts a call is inside, and who is waiting for them.
    ///
    /// An actor is **reentrant**: it releases its isolation at every
    /// `await`, so two calls for one account would otherwise both be
    /// inside `use` at once and both write to the same socket. IMAP
    /// interleaves untagged responses with tagged ones, so two commands
    /// in flight on one connection is a parser reading somebody else's
    /// answer. Actor isolation does not give exclusion across an
    /// `await`; this does.
    private var busy: Set<String> = []
    private var waiting: [String: [CheckedContinuation<Void, Never>]] = [:]

    init(
        open: @escaping @Sendable (String, UInt16) -> IMAPSession = {
            IMAPSession(host: $0, port: $1)
        },
        now: @escaping @Sendable () -> TimeInterval = { Date().timeIntervalSince1970 }
    ) {
        self.open = open
        self.now = now
    }

    /// Run `body` against a signed-in session for `account`.
    ///
    /// Exclusive for the length of the call — see ``busy``.
    func use<T: Sendable>(
        _ account: MailAccount, secret: String,
        _ body: sending (IMAPSession) async throws -> T
    ) async throws -> T {
        await acquire(account.id)
        defer { release(account.id) }
        let session: IMAPSession
        if let kept = try await reusable(account) {
            session = kept
        } else {
            session = try await freshly(account, secret: secret)
            held[account.id] = Held(session: session, lastUsed: now())
        }
        do {
            let out = try await body(session)
            // Only if it is still there: `drop` may have run while this
            // was in flight — the account removed, the socket closed
            // under the command — and putting it back would resurrect a
            // connection signed in with a credential that is gone.
            if held[account.id] != nil {
                held[account.id] = Held(session: session, lastUsed: now())
            }
            return out
        } catch {
            // Not put back. A session that threw may be mid-command,
            // with an answer still to arrive, and the next caller would
            // read it as its own.
            held[account.id] = nil
            await session.close()
            throw error
        }
    }

    /// Close and forget this account's connection, if it has one.
    func drop(_ accountId: String) async {
        guard let gone = held.removeValue(forKey: accountId) else { return }
        await gone.session.close()
    }

    /// Close and forget every connection.
    ///
    /// **No production caller, deliberately.** Removing an account
    /// closes that account's connection, which is the case that
    /// matters; there is no sign-out-everything in the app, and a
    /// lifecycle hook to close on backgrounding would be a second
    /// answer to a question the freshness probe already answers.
    ///
    /// It exists so a test can put its pool back before the next one
    /// runs. Stated here rather than left to be noticed, because a
    /// method nobody calls is usually a feature that is off.
    func dropAll() async {
        for id in held.keys { await drop(id) }
    }

    /// How many connections are being kept. Reads state; changes none.
    func count() -> Int { held.count }

    // MARK: - exclusion

    private func acquire(_ id: String) async {
        while busy.contains(id) {
            await withCheckedContinuation { waiting[id, default: []].append($0) }
        }
        busy.insert(id)
    }

    private func release(_ id: String) {
        busy.remove(id)
        guard var queue = waiting[id], !queue.isEmpty else { return }
        let next = queue.removeFirst()
        waiting[id] = queue
        next.resume()
    }

    // MARK: - connections

    /// The kept connection, if there is one and it answers.
    ///
    /// Read rather than taken: it stays in ``held`` for the whole call,
    /// so ``drop(_:)`` can still find and close it if the account is
    /// removed while this one is in flight.
    private func reusable(_ account: MailAccount) async throws -> IMAPSession? {
        guard let candidate = held[account.id] else { return nil }
        if now() - candidate.lastUsed <= ImapPool.freshWindow { return candidate.session }
        do {
            try await candidate.session.noop()
            return candidate.session
        } catch {
            held[account.id] = nil
            await candidate.session.close()
            return nil
        }
    }

    private func freshly(_ account: MailAccount, secret: String) async throws -> IMAPSession {
        let session = open(account.imapHost, account.imapPort)
        do {
            try await session.connect()
            if account.auth == .oauth2 {
                try await session.authenticateXOAuth2(user: account.loginName, token: secret)
            } else {
                try await session.login(user: account.loginName, password: secret)
            }
        } catch {
            await session.close()
            throw error
        }
        return session
    }
}
