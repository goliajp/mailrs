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
        }
        .sheet(isPresented: $composing) { ComposeView().frame(minWidth: 640, minHeight: 520) }
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
            }
            .accessibilityIdentifier("mac.conversations")
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
                    Label(list.title, systemImage: list.systemImage)
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
