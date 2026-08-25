import SwiftUI

/// The iPad's own layout: three columns, not a phone stretched.
///
/// A mail client on a 13-inch screen showing one list at a time, with
/// the message it was opened from nowhere, is the thing Apple's
/// large-screen guidance is about — and it is what this app did on an
/// iPad until now, because `ConversationListView` is a
/// `NavigationStack` and nothing branched on width.
///
/// The three columns are the shape every mail client on this device
/// settles on, for a reason worth stating: the sidebar answers *which
/// mailbox*, the middle answers *which conversation*, and the detail
/// answers *what it says*. Collapsing any two of those means one of
/// those questions has to be re-asked by navigating.
///
/// **The phone is untouched.** `RootView` picks between this and
/// `ConversationListView` on the horizontal size class, so an iPad in
/// Slide Over gets the phone design — which is the right design for
/// that width — and the phone's own screens, and the tests over them,
/// are the same as they were.
struct PadRootView: View {
    @Environment(Session.self) private var session
    @Environment(Preferences.self) private var preferences
    @Environment(\.theme) private var theme

    /// Which conversation the detail column is showing.
    ///
    /// Held here rather than by the list, because on this layout the
    /// detail is a *sibling* of the list rather than something pushed
    /// on top of it — that is the whole difference from the phone.
    @State private var opened: Wire.Conversation?
    @State private var searchText = ""

    private var listTitle: String {
        LocalisedTitle.of(
            session.activeList.titleKey,
            in: preferences.language.locale ?? Locale.autoupdatingCurrent)
    }
    @State private var searchTask: Task<Void, Never>?
    @State private var composing = false
    @State private var showingSettings = false
    @State private var confirmingMarkAllRead = false
    @State private var columns = NavigationSplitViewVisibility.all
    /// Held rather than deleted at once: the phone asks, and a bigger
    /// screen is not a reason to stop asking.
    @State private var pendingDelete: Wire.Conversation?

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            PadSidebar(showingSettings: $showingSettings)
        } content: {
            conversations
                // Resolved in code — see `LocalisedTitle`. A key handed
                // to `navigationTitle` is resolved in the window's
                // title bar, outside this app's `\.locale` override,
                // and the window then shows two languages at once.
                .navigationTitle(listTitle)
                .searchable(text: $searchText, prompt: Text("Search mail"))
                .onChange(of: searchText) { _, text in search(text) }
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button {
                            composing = true
                        } label: {
                            Label("New message", systemImage: "square.and.pencil")
                        }
                        .accessibilityIdentifier("pad.compose")
                        .keyboardShortcut("n", modifiers: .command)
                    }
                    ToolbarItem(placement: .topBarTrailing) {
                        Button {
                            Task { await session.loadConversations() }
                        } label: {
                            Label("Fetch mail", systemImage: "arrow.clockwise")
                        }
                        .accessibilityIdentifier("pad.sync")
                        .keyboardShortcut("r", modifiers: .command)
                    }
                    // Emptying the unread count in one go, which the
                    // phone has offered from its list menu since the
                    // beginning and this device could not do at all —
                    // the only way to clear a mailbox here was one row
                    // at a time.
                    ToolbarItem(placement: .topBarTrailing) {
                        Button {
                            confirmingMarkAllRead = true
                        } label: {
                            Label("Mark all as read", systemImage: "envelope.open")
                        }
                        .accessibilityIdentifier("pad.markAllRead")
                    }
                }
        } detail: {
            detail
        }
        // Balanced, not prominent: on this app the middle column is
        // read as much as the detail — somebody triaging spends their
        // time there — and `.prominentDetail` squeezes it to make room
        // for a message they have not chosen yet.
        .navigationSplitViewStyle(.balanced)
        // Over the middle column, which is the one the rows left from
        // — an undo floating over the message being read would point
        // at the wrong place.
        // Asked before it happens: marking a mailbox read cannot be
        // undone and changes rows the reader cannot see.
        .alert("Mark all as read?", isPresented: $confirmingMarkAllRead) {
            Button("Mark all as read") { Task { await session.markListRead() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Every conversation in \(listTitle) will be marked read.")
        }
        .overlay(alignment: .bottom) { UndoBar() }
        // Failures were silent on this screen: `session.banner` is set
        // and nobody was reading it, so an archive the server refused
        // looked like an archive that worked.
        .overlay(alignment: .top) { FailureBanner() }
        .animation(.easeOut(duration: 0.2), value: session.banner)
        .animation(.easeOut(duration: 0.2), value: session.pendingUndo != nil)
        .onChange(of: session.activeList) { _, _ in dropSelectionIfGone() }
        .onChange(of: session.visibleConversations.count) { _, _ in dropSelectionIfGone() }
        .sheet(isPresented: $composing) { ComposeView() }
        .confirmationDialog(
            "Delete this conversation?",
            isPresented: .constant(pendingDelete != nil),
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) {
                if let doomed = pendingDelete {
                    Task { await session.delete(doomed) }
                }
                pendingDelete = nil
            }
            Button("Cancel", role: .cancel) { pendingDelete = nil }
        }
        .sheet(isPresented: $showingSettings) { SettingsView() }
        .task { await session.loadConversations() }
    }

    @ViewBuilder private var conversations: some View {
        if session.activeList == .send {
            SendListSection(searchText: searchText)
        } else if session.visibleConversations.isEmpty {
            if session.initialLoading {
                ProgressView()
            } else if session.searchQuery != nil {
                ContentUnavailableView.search(text: searchText)
            } else {
                ContentUnavailableView(
                    session.activeList.emptyMessage,
                    systemImage: session.activeList.systemImage)
            }
        } else {
            // Selection drives the detail column. `List(selection:)`
            // rather than `NavigationLink`, because a link pushes and
            // this layout does not push — the whole point is that the
            // list stays where it is.
            List(session.visibleConversations, selection: selectionBinding) { conversation in
                ConversationRow(conversation: conversation, isSelecting: false)
                    .tag(conversation.threadId)
                    // Paging. Without it the list simply ends at the
                    // first page, and a mailbox with more in it looks
                    // like a mailbox that does not.
                    .onAppear {
                        if conversation.threadId == session.visibleConversations.last?.threadId {
                            Task { await session.loadMore() }
                        }
                    }
                    .contextMenu { ConversationRowMenu(conversation: conversation) }
                    // **The same swipes the phone has.** Converting
                    // these rows from `NavigationLink` to a selection
                    // dropped them, and losing archive-by-swipe is
                    // losing the gesture triage is actually done with —
                    // on the device with the most room to do it.
                    .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                        Button {
                            Task { await session.archive(conversation) }
                        } label: {
                            Label("Archive", systemImage: "archivebox")
                        }
                        .tint(.green)
                        Button(role: .destructive) {
                            pendingDelete = conversation
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .swipeActions(edge: .leading, allowsFullSwipe: true) {
                        Button {
                            Task { await session.toggleRead(conversation) }
                        } label: {
                            Label(
                                ReadToggle.label(unread: conversation.unreadCount > 0),
                                systemImage: ReadToggle.icon(
                                    unread: conversation.unreadCount > 0))
                        }
                        .tint(.blue)
                    }
            }
            .accessibilityIdentifier("pad.conversations")
            // The gesture people reach for without being told. The
            // phone has had it since the beginning; leaving it off the
            // iPad meant the only way to fetch was a button somebody
            // had to find.
            .refreshable { await session.loadConversations() }
            // A hardware keyboard is ordinary on this device. ↑↓ walk
            // the list without lifting a hand to the screen, which is
            // how somebody with a keyboard case reads mail.
            .onKeyPress(.upArrow) { step(-1) }
            .onKeyPress(.downArrow) { step(1) }
        }
    }

    @ViewBuilder private var detail: some View {
        if let opened {
            ThreadView(conversation: opened)
                // Keyed on the thread, so choosing another conversation
                // builds a new ThreadView rather than handing the old
                // one a different `conversation` and hoping every piece
                // of its state notices.
                .id(opened.threadId)
        } else {
            // Said, not blank. An empty pane with no explanation reads
            // as something that failed to load.
            ContentUnavailableView(
                "No conversation selected",
                systemImage: "envelope.open",
                description: Text("Choose a conversation to read it here."))
            .accessibilityIdentifier("pad.noSelection")
        }
    }

    /// Forget what is open when it no longer belongs to what is
    /// listed.
    ///
    /// Switching mailbox or typing a search leaves the detail column
    /// showing a conversation that is not in the list beside it —
    /// opened from Inbox, still on screen under Archived, and not
    /// reachable in the list any more. The phone cannot have this: it
    /// pushes, so leaving the list means leaving the message.
    ///
    /// Compared against the rows rather than cleared unconditionally:
    /// a conversation that survives the change — the same message
    /// found by a search, say — should stay open rather than blink
    /// away and make somebody find it again.
    private func dropSelectionIfGone() {
        guard let open = opened else { return }
        let stillListed = session.visibleConversations.contains { $0.threadId == open.threadId }
        if !stillListed { opened = nil }
    }

    private var selectionBinding: Binding<String?> {
        Binding(
            get: { opened?.threadId },
            set: { id in
                opened = session.visibleConversations.first { $0.threadId == id }
            })
    }

    /// Move the selection by one, and open what it lands on.
    ///
    /// Returns `.ignored` at the ends so the key press falls through
    /// to whatever else wants it, rather than being swallowed by a
    /// list that cannot move — a keyboard that stops responding at the
    /// end of a list reads as the app having hung.
    private func step(_ by: Int) -> KeyPress.Result {
        let rows = session.visibleConversations
        guard !rows.isEmpty else { return .ignored }
        guard let current = opened.flatMap({ o in rows.firstIndex { $0.threadId == o.threadId } })
        else {
            opened = rows.first
            return .handled
        }
        let next = current + by
        guard rows.indices.contains(next) else { return .ignored }
        opened = rows[next]
        return .handled
    }

    private func search(_ text: String) {
        searchTask?.cancel()
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        searchTask = Task {
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            await session.search(text: trimmed)
        }
    }
}
