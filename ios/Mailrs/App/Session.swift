import Foundation
import Observation

/// Who is signed in, and the client that speaks for them.
///
/// `@Observable` rather than `ObservableObject`: the app targets iOS 18,
/// where Observation is the supported shape and a view only re-renders
/// for the properties it actually reads.
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
    /// A launch argument overrides it, which is how the UI tests point
    /// the app at a stub instead of a live server. Inert in normal use —
    /// nothing passes this flag but a test runner.
    var baseURL: URL = {
        if let index = ProcessInfo.processInfo.arguments.firstIndex(of: "-mailrsBaseURL"),
           index + 1 < ProcessInfo.processInfo.arguments.count,
           let url = URL(string: ProcessInfo.processInfo.arguments[index + 1]) {
            return url
        }
        return URL(string: "https://mail.golia.jp")!
    }()
    private(set) var state: State = .signedOut
    private(set) var conversations: [Wire.Conversation] = []
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
            conversations = (try? await client.conversations()) ?? []
            return
        }
        guard let token = TokenStore.load() else { return }
        let client = MailrsClient(baseURL: baseURL, token: token)
        self.client = client
        state = .signedIn(address: TokenStore.loadAddress() ?? "", displayName: "")
        do {
            conversations = try await client.conversations()
        } catch {
            // The stored token no longer works — clear it rather than
            // leaving a credential that fails on every launch.
            TokenStore.clear()
            self.client = nil
            state = .signedOut
        }
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
            conversations = try await client.conversations()
        } catch {
            state = .failed(error.localizedDescription)
        }
    }
}
