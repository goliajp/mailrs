import Foundation
import Observation
import SwiftUI

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

        ?? URL(string: "https://mail.golia.ai")!
    var state: State = .signedOut

    // `private(set)` would make the *setter* file-private, and these
    // are written from Session+Compose / +Directory, which are the
    // same type in another file. Swift has no "settable within the
    // type" — so the compiler no longer forbids a view assigning
    // these, and nothing does.
    var conversations: [Wire.Conversation] = []

    /// Whether the server has any older threads left.
    var reachedEnd = false

    var loadingMore = false

    /// True from asking for a list until its first page answers.
    ///
    /// Its whole purpose is the empty state: without it, "All caught up"
    /// flashes on every cold open and every list switch while the first
    /// page is still in flight — an empty mailbox announced about a full
    /// one. Apple Mail never shows the empty state until it knows.
    var initialLoading = false

    /// Last-known rows on disk; every successful fetch overwrites.
    let cache = MailCache.bootstrap()

    /// The query the visible rows answer, or nil when they are the list.
    /// A request that failed while signed in.
    ///
    /// Not `state = .failed`: `RootView` shows `SignInView` for every
    /// state that is not `.signedIn`, so a refused search or a failed
    /// star threw the reader out of the mailbox and printed the error
    /// on the sign-in form as a red button. Reported from the phone as
    /// searching sometimes logging you out, and as a red button on the
    /// login page saying something had been cancelled. Signing in is
    /// the one failure that belongs on that screen; the rest belong
    /// over the mailbox they happened in.
    var banner: String?

    var searchQuery: String?

    var searchResults: [Wire.Conversation] = []


    /// What the list draws — search results while searching, the mailbox
    /// otherwise. One property so no screen has to know which it is
    /// looking at, and no screen can show one while counting the other.
    /// The signed-in address, lowercased for comparison — the "me" the
    /// row-face rule filters out.
    var myAddress: String {
        if case let .signedIn(address, _) = state { return address.lowercased() }
        return ""
    }


    var visibleConversations: [Wire.Conversation] {
        if searchQuery == nil { return conversations }
        return searchResults
    }

    var needsTotp = false


    // Stored properties cannot live in an extension, so the state the
    // split subjects act on stays here with the type. Their methods are
    // in Session+Directory / +Triage / +Compose / +Operations.


    var drafts: [Wire.Draft] = []


    /// Why the last load failed, if it did.
    ///
    /// `(try? …) ?? []` said "you have no drafts" when the server had
    /// said nothing at all — the one swallowed error in this file that
    /// the reader would have believed, because an empty draft list is
    /// perfectly ordinary. A failure now keeps the previous drafts and
    /// says so.
    var draftsFailure: String?

    var sendRows: [SendJoin.Row] = []



    /// The aliases that deliver to the signed-in address.
    ///
    /// Held so a thread can say which of my addresses a message actually
    /// arrived at. Loaded once per session and not refreshed: aliases
    /// change on the order of never, and a mail app that polls the
    /// directory is spending someone's battery on it.
    var myAliases: [Wire.Alias] = []



    var pendingUndo: PendingUndo?

    var undoDismissTask: Task<Void, Never>?

    // `private(set)`: the moved extensions read it, and Swift's
    // `private` would hide it from a file that is still this type.
    // Setting it stays here, with signing in and out.
    private(set) var client: MailrsClient?


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
            state = .signedIn(address: "me@golia.jp", displayName: "Test")
            // Through loadConversations, not an inline fetch: the badge
            // refresh lives there, and this path quietly skipping it is
            // exactly how the real cold launch below shipped a badge
            // that never updated.
            await loadConversations()
            await loadMyAliases()
            return
        }
        guard let token = TokenStore.load() else { return }
        let client = MailrsClient(baseURL: baseURL, token: token)
        self.client = client
        state = .signedIn(address: TokenStore.loadAddress() ?? "", displayName: "")
        do {
            conversations = try await client.conversations(axes: axes)
            await refreshBadge()
            await loadMyAliases()
        } catch {
            // The stored token no longer works — clear it rather than
            // leaving a credential that fails on every launch.
            TokenStore.clear()
            self.client = nil
            state = .signedOut
        }
    }


    /// A sender domain's brand icon, or nil when there is none.
    func icon(domain: String) async -> Data? {
        guard let client else { return nil }
        return try? await client.icon(domain: domain)
    }


    /// Contact suggestions for a To field. Failures answer as no
    /// suggestions — autocomplete is an offer, not a feature that may
    /// interrupt composing with an error.
    func contacts(matching query: String) async -> [String] {
        guard let client else { return [] }
        return (try? await client.contacts(matching: query)) ?? []
    }


    func attachment(uid: UInt32, index: Int) async throws -> Data {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.attachment(uid: uid, index: index)
    }


    /// Upload this device's APNs token, so the server can reach it.
    func registerPushToken(_ token: String) async {
        guard let client else { return }
        try? await client.registerPushToken(token)
    }


    func messages(threadId: String) async throws -> [Wire.Message] {
        guard let client else { throw MailrsError.badCredentials }
        let fresh = try await client.messages(threadId: threadId)
        cache.writeMessages(fresh, threadId: threadId)
        return fresh
    }


    /// The last fetch of this thread, from disk — what an opened
    /// conversation shows while (or without) the network answering.
    func cachedMessages(threadId: String) -> [Wire.Message]? {
        cache.readMessages(threadId: threadId)
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
            // Remembered for the next sign-in: who it was, and behind
            // Face ID, what to send. Kept on the address the server
            // answered with rather than the one that was typed, so a
            // login by alias stores itself under the account it reached.
            CredentialStore.lastAddress = login.address
            CredentialStore.save(password: password, address: login.address)
            state = .signedIn(address: login.address, displayName: login.displayName)
            // The badge only — see `PushRegistrar`.
            PushRegistrar.requestBadgeAuthorization()
            await loadConversations()
            await loadMyAliases()
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
        // A cold screen opens on the last-known rows while the network
        // answers — a mailbox you saw an hour ago beats a spinner. The
        // fetch then replaces them in place; only a screen with nothing
        // at all shows the spinner, and pull-to-refresh on a populated
        // list still never blanks it.
        if conversations.isEmpty, let cached = cache.readConversations(list: activeList.rawValue) {
            withAnimation { conversations = cached }
        }
        if conversations.isEmpty { initialLoading = true }
        do {
            let page = try await client.conversations(axes: axes)
            // Rows slide in and the empty state cross-fades away, the
            // way a List is expected to move. Without the animation
            // block SwiftUI swaps the frames in one step.
            withAnimation { conversations = page }
            reachedEnd = false
            cache.writeConversations(page, list: activeList.rawValue)
        } catch {
            // With cached rows on screen, a failed refresh is stale
            // mail, not a broken app — the error state would replace a
            // readable mailbox with an apology.
            if conversations.isEmpty {
                banner = error.localizedDescription
            }
        }
        initialLoading = false
        await refreshBadge()
    }


    /// What a return to the app asks for.
    ///
    /// The list as it stands now, plus the badge. Not while a search is
    /// on screen: the rows behind it are not what the reader is
    /// looking at, and replacing them under a result set is a change
    /// nobody asked for.
    func refreshForeground() async {
        guard case .signedIn = state, searchQuery == nil else { return }
        await loadConversations()
    }


    /// The icon's number, refreshed wherever the mailbox may have moved:
    /// after a list load, after marking read, after a delete. Server
    /// count, because the client only ever holds one page of one list.
    func refreshBadge() async {
        guard let client else { return }
        if let count = try? await client.unseenCount() {
            AppBadge.update(count)
        }
    }


    /// Switch lists.
    ///
    /// Everything scoped to the old list goes with it: the rows, the
    /// paging cursor's end flag, and any search. Carrying a search across
    /// would leave Junk showing hits from Inbox, and carrying `reachedEnd`
    /// would leave a fresh list unable to page.
    func select(_ list: MailList) async {
        clearUndo()
        guard list != activeList else { return }
        activeList = list
        // The new list's cached rows, in the same breath as clearing the
        // old ones — and `initialLoading` set here rather than inside
        // the load.
        //
        // `loadConversations` is awaited, and the await yields: for that
        // frame the rows were empty and nothing said a load was running,
        // so SwiftUI drew the empty state. Every switch to an uncached
        // list flashed "All caught up" at a mailbox that was about to
        // fill. Reported from the phone as loading and empty being
        // confused, which is what it is — the view was asked a question
        // it did not yet have the evidence to answer.
        let cached = cache.readConversations(list: list.rawValue) ?? []
        conversations = cached
        initialLoading = cached.isEmpty
        searchResults = []
        searchQuery = nil
        reachedEnd = false
        sendRows = []
        if list == .send {
            initialLoading = true
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
            withAnimation {
                searchQuery = nil
                searchResults = []
            }
            return
        }
        searchQuery = query
        do {
            let hits = try await client.search(query: query, axes: axes)
            // Only if this is still the current query — a slower earlier
            // request must not overwrite a later one's results.
            guard searchQuery == query else { return }
            withAnimation { searchResults = hits }
        } catch {
            banner = error.localizedDescription
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
            banner = error.localizedDescription
        }
    }
}

