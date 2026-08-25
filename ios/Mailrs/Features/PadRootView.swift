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
    @State private var columns = NavigationSplitViewVisibility.all

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
                }
        } detail: {
            detail
        }
        // Balanced, not prominent: on this app the middle column is
        // read as much as the detail — somebody triaging spends their
        // time there — and `.prominentDetail` squeezes it to make room
        // for a message they have not chosen yet.
        .navigationSplitViewStyle(.balanced)
        .sheet(isPresented: $composing) { ComposeView() }
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
                    .contextMenu { ConversationRowMenu(conversation: conversation) }
            }
            .accessibilityIdentifier("pad.conversations")
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

    private var selectionBinding: Binding<String?> {
        Binding(
            get: { opened?.threadId },
            set: { id in
                opened = session.visibleConversations.first { $0.threadId == id }
            })
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
