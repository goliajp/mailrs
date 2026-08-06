import Foundation
import Observation

/// Who is signed in, and the client that speaks for them.
///
/// `@Observable` rather than `ObservableObject`: the app targets iOS 18,
/// where Observation is the supported shape and a view only re-renders
/// for the properties it actually reads.
/// Reads a `-flag value` pair off the launch arguments.
///
/// Free function rather than a static member so stored-property
/// initialisers in `Session` can call it.
private func launchValue(_ flag: String) -> String? {
    let arguments = ProcessInfo.processInfo.arguments
    guard let index = arguments.firstIndex(of: flag), index + 1 < arguments.count else {
        return nil
    }
    return arguments[index + 1]
}

@Observable
@MainActor
final class Session {
    enum State: Equatable {
        case signedOut
        case signingIn
        case signedIn(address: String, displayName: String)
        case failed(String)
    }

    /// Where this app points. A simulator reaches the host's localhost
    /// directly, so the local stack works with no extra plumbing; a
    /// device needs the LAN address or the public host instead.
    /// Which folder the list asks for. A launch argument overrides it so
    /// a UI test can reach the stub's paging fixture; the app itself has
    /// no folder switcher yet.
    /// Not `Self.launchValue(…)`: a stored property initialiser cannot
    /// reference the covariant `Self` of a non-final class, and the
    /// `@Observable` macro expands this into one.
    var folder: String = launchValue("-mailrsFolder") ?? "Inbox"

    /// A launch argument overrides it, which is how the UI tests point
    /// the app at a stub instead of a live server. Inert in normal use —
    /// nothing passes this flag but a test runner.
    var baseURL: URL = launchValue("-mailrsBaseURL").flatMap(URL.init(string:))
        ?? URL(string: "https://mail.golia.jp")!
    private(set) var state: State = .signedOut
    private(set) var conversations: [Wire.Conversation] = []
    /// Whether the server has any older threads left.
    private(set) var reachedEnd = false
    private(set) var loadingMore = false
    private(set) var needsTotp = false

    private var client: MailrsClient?

    /// Sign back in from the Keychain, if a token is there.
    ///
    /// Optimistic: the token is used, and a 401 on the first real request
    /// sends the user back to the sign-in screen. The alternative is a
    /// round trip before the UI can decide what to draw, which shows a
    /// login form to an already-signed-in user every cold launch.
    func restore() async {
        // A UI test starts from a known state. Without this the Keychain
        // token from the previous test survives, the app restores
        // straight into the inbox, and the next test looks for a
        // sign-in form that is not there — which is exactly how the
        // first run of these tests failed.
        if ProcessInfo.processInfo.arguments.contains("-mailrsSignedOut") {
            TokenStore.clear()
            return
        }
        // A UI test whose subject is not the login screen starts already
        // signed in. Not only for speed: typing into a `.password` field
        // makes iOS offer to save it, and that prompt is drawn by a
        // system process — not by this app and not by SpringBoard — so it
        // sits over the inbox where no test can dismiss it and every row
        // underneath is genuinely untappable.
        if let index = ProcessInfo.processInfo.arguments.firstIndex(of: "-mailrsToken"),
           index + 1 < ProcessInfo.processInfo.arguments.count {
            let client = MailrsClient(baseURL: baseURL, token: ProcessInfo.processInfo.arguments[index + 1])
            self.client = client
            state = .signedIn(address: "test@golia.jp", displayName: "Test")
            conversations = (try? await client.conversations(folder: folder)) ?? []
            return
        }
        guard let token = TokenStore.load() else { return }
        let client = MailrsClient(baseURL: baseURL, token: token)
        self.client = client
        state = .signedIn(address: TokenStore.loadAddress() ?? "", displayName: "")
        do {
            conversations = try await client.conversations(folder: folder)
        } catch {
            // The stored token no longer works — clear it rather than
            // leaving a credential that fails on every launch.
            TokenStore.clear()
            self.client = nil
            state = .signedOut
        }
    }

    func sendReply(
        to recipients: [String], subject: String, body: String,
        inReplyTo: String?, threadId: String
    ) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.sendReply(
            to: recipients, subject: subject, body: body,
            inReplyTo: inReplyTo, threadId: threadId
        )
    }

    func messages(threadId: String) async throws -> [Wire.Message] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.messages(threadId: threadId)
    }

    func signIn(address: String, password: String, totpCode: String?) async {
        state = .signingIn
        needsTotp = false
        let client = MailrsClient(baseURL: baseURL)
        do {
            let login = try await client.logIn(
                address: address, password: password, totpCode: totpCode
            )
            self.client = client
            TokenStore.save(login.token)
            TokenStore.saveAddress(login.address)
            state = .signedIn(address: login.address, displayName: login.displayName)
            await loadConversations()
        } catch MailrsError.needsTotp {
            needsTotp = true
            state = .signedOut
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    func signOut() {
        TokenStore.clear()
        client = nil
        conversations = []
        state = .signedOut
    }

    func loadConversations() async {
        guard let client else { return }
        do {
            conversations = try await client.conversations(folder: folder)
            reachedEnd = false
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    /// The next page, keyed off the oldest row on screen.
    ///
    /// Guarded against re-entry as well as against the end: the list asks
    /// for more when its last row appears, and that row stays on screen
    /// while the request is in flight.
    func loadMore() async {
        guard let client, !loadingMore, !reachedEnd else { return }
        guard let before = ThreadPage.nextBefore(after: conversations) else { return }
        loadingMore = true
        defer { loadingMore = false }
        do {
            let page = try await client.conversations(folder: folder, before: before)
            let merged = ThreadPage.merge(conversations, with: page)
            conversations = merged.rows
            // Nothing new means the end — including the case where a page
            // came back entirely full of the boundary second's rows.
            if !merged.progressed { reachedEnd = true }
        } catch {
            // A failed page is not the end of the mailbox; leave the flag
            // alone so pulling again retries.
            state = .failed(error.localizedDescription)
        }
    }
}
