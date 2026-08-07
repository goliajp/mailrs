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
    /// The list on screen. Everything the list, its paging and its search
    /// ask for comes from this one value.
    private(set) var activeList: MailList = .inbox

    /// Points the list at the stub's paging fixture, which is not one of
    /// the real lists. Not `Self.launchValue(…)`: a stored property
    /// initialiser cannot reference the covariant `Self` of a non-final
    /// class, and the `@Observable` macro expands this into one.
    private let folderOverride: String? = launchValue("-mailrsFolder")

    var axes: MailListAxes {
        if let folderOverride { return MailListAxes(folder: folderOverride) }
        return activeList.axes
    }

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
    /// The query the visible rows answer, or nil when they are the list.
    private(set) var searchQuery: String?
    private(set) var searchResults: [Wire.Conversation] = []

    /// What the list draws — search results while searching, the mailbox
    /// otherwise. One property so no screen has to know which it is
    /// looking at, and no screen can show one while counting the other.
    var visibleConversations: [Wire.Conversation] {
        searchQuery == nil ? conversations : searchResults
    }
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
            conversations = (try? await client.conversations(axes: axes)) ?? []
            return
        }
        guard let token = TokenStore.load() else { return }
        let client = MailrsClient(baseURL: baseURL, token: token)
        self.client = client
        state = .signedIn(address: TokenStore.loadAddress() ?? "", displayName: "")
        do {
            conversations = try await client.conversations(axes: axes)
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

    /// A thread is read because someone is reading it.
    ///
    /// Called by the thread view once its messages are on screen — not
    /// by the list, and not on selection. The web client learned that
    /// distinction the hard way: a hidden pane auto-opening the newest
    /// thread marked mail read that had never been displayed. Here the
    /// view only exists when a person navigated into it, which is what
    /// makes this safe.
    func markThreadRead(_ conversation: Wire.Conversation) async {
        guard conversation.unreadCount > 0, let client else { return }
        do {
            try await client.setRead(threadId: conversation.threadId, true)
            // Patched after the server confirms, and the row is not
            // re-filtered: in the Unread list the row stays visible
            // until the next refresh rather than vanishing while you
            // are standing on it.
            patch(conversation.threadId) { $0.unreadCount = 0 }
        } catch {
            // Still unread is the honest state if the call failed; the
            // next open retries by construction.
        }
    }

    /// Toggle read, and show it immediately.
    ///
    /// Optimistic, like archive and unlike delete: both directions are
    /// reversible by the same gesture, so the worst a failed call costs
    /// is a row that snaps back.
    func toggleRead(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        let markRead = conversation.unreadCount > 0
        let previous = conversations
        patch(conversation.threadId) { $0.unreadCount = markRead ? 0 : max(1, $0.unreadCount) }
        do {
            try await client.setRead(threadId: conversation.threadId, markRead)
        } catch {
            conversations = previous
            state = .failed(error.localizedDescription)
        }
    }

    func toggleStarred(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        let starred = !conversation.flagged
        let previous = conversations
        patch(conversation.threadId) { $0.flagged = starred }
        do {
            try await client.setStarred(threadId: conversation.threadId, starred)
        } catch {
            conversations = previous
            state = .failed(error.localizedDescription)
        }
    }

    /// Replace one row in whichever collection is on screen.
    ///
    /// Both, not whichever is showing: a row can be in the list and in
    /// the search results at once, and patching only the visible one
    /// leaves the other holding the old value for when the search is
    /// dismissed.
    private func patch(_ threadId: String, _ change: (inout Wire.Conversation) -> Void) {
        if let index = conversations.firstIndex(where: { $0.threadId == threadId }) {
            change(&conversations[index])
        }
        if let index = searchResults.firstIndex(where: { $0.threadId == threadId }) {
            change(&searchResults[index])
        }
    }

    /// Mark junk (or rescue from Junk), and take the row off this list.
    ///
    /// Optimistic like archive: the same verb in the other direction
    /// undoes it, and the worst a failure costs is a row that returns.
    /// The row leaves whichever list is showing because the verdict
    /// moves the thread between folders — a spam row lingering in the
    /// Inbox after being marked is the confusing outcome.
    func setJunk(_ conversation: Wire.Conversation, junk: Bool) async {
        guard let client else { return }
        let previous = conversations
        let previousResults = searchResults
        conversations.removeAll { $0.threadId == conversation.threadId }
        searchResults.removeAll { $0.threadId == conversation.threadId }
        do {
            try await client.setJunk(threadId: conversation.threadId, junk)
        } catch {
            conversations = previous
            searchResults = previousResults
            state = .failed(error.localizedDescription)
        }
    }

    /// Archive, and take the row off the list.
    ///
    /// Optimistic, because archiving is reversible: if the server refuses
    /// the row comes back, and the worst case is a row that reappears
    /// rather than mail that is gone.
    func archive(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        let previous = conversations
        conversations.removeAll { $0.threadId == conversation.threadId }
        do {
            try await client.archive(threadId: conversation.threadId)
        } catch {
            conversations = previous
            state = .failed(error.localizedDescription)
        }
    }

    /// Delete, and take the row off the list.
    ///
    /// Not optimistic. The server unlinks the maildir files, so there is
    /// nothing to restore and no honest way to put the row back — the
    /// row goes only once the server says it is gone.
    func delete(_ conversation: Wire.Conversation) async {
        guard let client else { return }
        do {
            try await client.delete(threadId: conversation.threadId)
            conversations.removeAll { $0.threadId == conversation.threadId }
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    func attachment(uid: UInt32, index: Int) async throws -> Data {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.attachment(uid: uid, index: index)
    }

    private(set) var drafts: [Wire.Draft] = []
    private(set) var sendRows: [SendJoin.Row] = []

    /// The Send list's rows: both endpoints, joined. Failures are the
    /// list's whole reason to exist, so a partial fetch does not render
    /// as an empty "Nothing sent yet".
    func loadSendRows() async {
        guard let client else { return }
        do {
            async let messages = client.sentMessages()
            async let sends = client.sends()
            sendRows = SendJoin.join(messages: try await messages, sends: try await sends)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    func loadDrafts() async {
        guard let client else { return }
        drafts = (try? await client.drafts()) ?? []
    }

    /// Save a compose session, returning the id the server gave it.
    ///
    /// The caller keeps that id and passes it back on the next save, so
    /// one session upserts one draft. Posting without it on every
    /// autosave would leave a new draft per tick.
    func saveDraft(
        id: Int64?, to: String, subject: String, body: String, replyToThreadId: String?
    ) async -> Int64? {
        guard let client else { return nil }
        let request = Wire.SaveDraftRequest(
            id: id, to: to, cc: "", bcc: "", subject: subject, body: body,
            replyToThreadId: replyToThreadId
        )
        return try? await client.saveDraft(request)
    }

    func deleteDraft(id: Int64) async {
        guard let client else { return }
        try? await client.deleteDraft(id: id)
        drafts.removeAll { $0.id == id }
    }

    /// Upload this device's APNs token, so the server can reach it.
    func registerPushToken(_ token: String) async {
        guard let client else { return }
        try? await client.registerPushToken(token)
    }

    /// Send a message that is not a reply.
    ///
    /// Both threading fields stay nil. Sending `reply_to_thread_id` here
    /// would file a new message inside an existing conversation, which
    /// is the mirror of the bug that made replies arrive unthreaded.
    func sendNew(to recipients: [String], subject: String, body: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.sendNew(to: recipients, subject: subject, body: body)
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
            PushRegistrar.requestAndRegister()
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
        // Send is not served by /api/conversations; asking it with empty
        // axes would answer with the whole mailbox.
        guard activeList != .send else { return await loadSendRows() }
        guard let client else { return }
        do {
            conversations = try await client.conversations(axes: axes)
            reachedEnd = false
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    /// Switch lists.
    ///
    /// Everything scoped to the old list goes with it: the rows, the
    /// paging cursor's end flag, and any search. Carrying a search across
    /// would leave Junk showing hits from Inbox, and carrying `reachedEnd`
    /// would leave a fresh list unable to page.
    func select(_ list: MailList) async {
        guard list != activeList else { return }
        activeList = list
        conversations = []
        searchResults = []
        searchQuery = nil
        reachedEnd = false
        sendRows = []
        if list == .send {
            await loadSendRows()
        } else {
            await loadConversations()
        }
    }

    /// Run a search, or clear one.
    ///
    /// The results replace the list rather than filtering it: the server
    /// searches the whole folder, so a client-side filter over the page
    /// in hand would miss everything below it.
    func search(text: String) async {
        guard let client else { return }
        guard let query = SearchRule.query(from: text) else {
            searchQuery = nil
            searchResults = []
            return
        }
        searchQuery = query
        do {
            let hits = try await client.search(query: query, axes: axes)
            // Only if this is still the current query — a slower earlier
            // request must not overwrite a later one's results.
            guard searchQuery == query else { return }
            searchResults = hits
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
        // Never while searching. Search results come back ranked, and
        // `before_ts` pages by date — asking for "older than the last
        // result" would append date-ordered rows to a relevance-ordered
        // list and quietly mix two orderings.
        guard searchQuery == nil else { return }
        guard let client, !loadingMore, !reachedEnd else { return }
        guard let before = ThreadPage.nextBefore(after: conversations) else { return }
        loadingMore = true
        defer { loadingMore = false }
        do {
            let page = try await client.conversations(axes: axes, before: before)
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
