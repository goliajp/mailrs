import SwiftUI

/// The Mac's shell: a source list, a message list, and the message.
///
/// The same three questions the iPad layout answers, asked the way
/// this platform asks them — a real source list rather than a
/// touch-sized sidebar, a unified toolbar carrying the actions, and
/// keyboard first. The columns are given widths a pointer can drag,
/// which a touch layout has no need to.
struct MacRootView: View {
    @Environment(Session.self) private var session
    @Environment(Preferences.self) private var preferences
    @Environment(\.colorScheme) private var colorScheme

    @State private var opened: Wire.Conversation?
    @State private var searchText = ""

    private var listTitle: String {
        LocalisedTitle.of(
            session.activeList.titleKey,
            in: preferences.language.locale ?? Locale.autoupdatingCurrent)
    }
    @State private var searchTask: Task<Void, Never>?
    @State private var composing = false
    @State private var pendingDelete: Wire.Conversation?

    var body: some View {
        Group {
            switch session.state {
            case .signedIn: mail
            default: SignInView()
            }
        }
        .environment(\.theme, Theme.of(colorScheme))
        .task { await session.restore() }
    }

    private var mail: some View {
        NavigationSplitView {
            MacSidebar()
                // A source list's width is the reader's to set, within
                // reason — the lower bound is the longest mailbox name
                // rather than a number that looked right on one screen.
                .navigationSplitViewColumnWidth(min: 180, ideal: 220, max: 320)
        } content: {
            conversations
                .navigationSplitViewColumnWidth(min: 280, ideal: 360, max: 520)
                // Resolved in code — see `LocalisedTitle`. A key handed
                // to `navigationTitle` is resolved in the window's
                // title bar, outside this app's `\.locale` override,
                // and the window then shows two languages at once.
                .navigationTitle(listTitle)
                .searchable(text: $searchText, placement: .toolbar, prompt: Text("Search mail"))
                .onChange(of: searchText) { _, text in search(text) }
        } detail: {
            detail
        }
        .onChange(of: session.activeList) { _, _ in dropSelectionIfGone() }
        .onChange(of: session.visibleConversations.count) { _, _ in dropSelectionIfGone() }
        .toolbar {
            ToolbarItemGroup {
                Button {
                    composing = true
                } label: {
                    Label("New message", systemImage: "square.and.pencil")
                }
                .help("New message (⌘N)")

                Button {
                    Task { await session.loadConversations() }
                } label: {
                    Label("Fetch mail", systemImage: "arrow.clockwise")
                }
                .help("Fetch mail (⌘R)")
            }

            // Actions on what is open. A Mac toolbar carries the verbs
            // for the selection; hiding them in a context menu means
            // right-clicking to do the thing the window is showing.
            ToolbarItemGroup {
                Button {
                    if let open = opened { Task { await session.archive(open) } }
                } label: {
                    Label("Archive", systemImage: "archivebox")
                }
                .disabled(opened == nil)
                // Named: "Archive" is also the word on the swipe action
                // and in the context menu, and a query on the label
                // matches all three.
                .accessibilityIdentifier("mac.toolbar.archive")
                .help("Archive (⌘E)")
                .keyboardShortcut("e", modifiers: .command)

                Button {
                    pendingDelete = opened
                } label: {
                    Label("Delete", systemImage: "trash")
                }
                .disabled(opened == nil)
                .accessibilityIdentifier("mac.toolbar.delete")
                .help("Delete (⌘⌫)")
                .keyboardShortcut(.delete, modifiers: .command)

                Button {
                    if let open = opened { Task { await session.toggleRead(open) } }
                } label: {
                    Label("Toggle read", systemImage: "envelope.badge")
                }
                .disabled(opened == nil)
                .accessibilityIdentifier("mac.toolbar.toggleRead")
                .help("Mark read or unread (⌘⇧U)")
                .keyboardShortcut("u", modifiers: [.command, .shift])
            }
        }
        .sheet(isPresented: $composing) { ComposeView().frame(minWidth: 640, minHeight: 520) }
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
        .onReceive(NotificationCenter.default.publisher(for: .macCompose)) { _ in
            composing = true
        }
        .onReceive(NotificationCenter.default.publisher(for: .macRefresh)) { _ in
            Task { await session.loadConversations() }
        }
        .task { await session.loadConversations() }
    }

    @ViewBuilder private var conversations: some View {
        if session.activeList == .send {
            SendListSection(searchText: searchText)
        } else if session.visibleConversations.isEmpty {
            if session.initialLoading {
                ProgressView()
            } else {
                ContentUnavailableView(
                    session.activeList.emptyMessage,
                    systemImage: session.activeList.systemImage)
            }
        } else {
            List(session.visibleConversations, selection: selectionBinding) { conversation in
                ConversationRow(conversation: conversation, isSelecting: false)
                    .tag(conversation.threadId)
                    .contextMenu { ConversationRowMenu(conversation: conversation) }
                    // Swipes exist on a trackpad too, and they are how
                    // the same triage is done on the other platforms.
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
            }
            .accessibilityIdentifier("mac.conversations")
            // Arrow keys walk the list. On this platform that is not a
            // nicety — a Mac app whose list cannot be driven from the
            // keyboard is one that has to be clicked through.
            .onKeyPress(.upArrow) { step(-1) }
            .onKeyPress(.downArrow) { step(1) }
        }
    }

    @ViewBuilder private var detail: some View {
        if let opened {
            ThreadView(conversation: opened).id(opened.threadId)
        } else {
            ContentUnavailableView(
                "No conversation selected",
                systemImage: "envelope.open",
                description: Text("Choose a conversation to read it here."))
            .accessibilityIdentifier("mac.noSelection")
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

    /// Move the selection by one. `.ignored` at the ends, so the key
    /// falls through rather than being swallowed by a list that cannot
    /// move — a keyboard that stops responding reads as a hang.
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

/// The Mac's source list.
///
/// `.sidebar` list style, which is what gives it the translucent
/// material and the selection shape every Mac app's first column has —
/// the iPad's inset-grouped rows in a window are the giveaway.
struct MacSidebar: View {
    @Environment(Session.self) private var session

    var body: some View {
        List(selection: selectionBinding) {
            Section("Mailboxes") {
                ForEach(MailList.allCases) { list in
                    SidebarMailboxRow(
                        list: list,
                        unread: list.badgeCount(
                            activeList: session.activeList,
                            unreadInActive: session.unreadInList))
                        .tag(list)
                        .accessibilityIdentifier("mac.list.\(list.rawValue)")
                }
            }
        }
        .listStyle(.sidebar)
        .accessibilityIdentifier("mac.sidebar")
    }

    private var selectionBinding: Binding<MailList?> {
        Binding(
            get: { session.activeList },
            set: { chosen in
                guard let chosen, chosen != session.activeList else { return }
                Task { await session.select(chosen) }
            })
    }
}
