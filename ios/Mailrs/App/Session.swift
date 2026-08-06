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
    var baseURL = URL(string: "https://mail.golia.jp")!
    private(set) var state: State = .signedOut
    private(set) var conversations: [Wire.Conversation] = []
    private(set) var needsTotp = false

    private var client: MailrsClient?

    func signIn(address: String, password: String, totpCode: String?) async {
        state = .signingIn
        needsTotp = false
        let client = MailrsClient(baseURL: baseURL)
        do {
            let login = try await client.logIn(
                address: address, password: password, totpCode: totpCode
            )
            self.client = client
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
