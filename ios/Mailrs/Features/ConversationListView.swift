import SwiftUI

struct ConversationListView: View {
    @Environment(Session.self) private var session
    /// The thread a delete is waiting on confirmation for.
    @State private var pendingDelete: Wire.Conversation?
    @State private var searchText = ""
    @State private var searchTask: Task<Void, Never>?
    @State private var composing = false
    @State private var showingDrafts = false
    @State private var confirmingMarkAllRead = false
    /// Multi-select state. `selection` only means anything while
    /// `editMode` is active; leaving select mode clears it.
    @State private var showingSettings = false
    @State private var selection = Set<String>()
    @State private var editMode: EditMode = .inactive
    /// The thread whose bucket is being chosen, from the swipe.
    @State private var pendingMove: Wire.Conversation?

    private var allSelected: Bool {
        !session.visibleConversations.isEmpty
            && selection.count == session.visibleConversations.count
    }
    /// The batch a delete is waiting on confirmation for.
    @State private var pendingBatchDelete: [Wire.Conversation]?

    var body: some View {
        NavigationStack {
            // The empty state replaces the list rather than covering it.
            // `.overlay { if isEmpty { … } }` reads well and installs a
            // full-screen container either way — with rows present it is
            // an invisible sheet of glass over them, and every row
            // reports itself untappable underneath it.
            Group {
                if session.activeList == .send {
                    SendListSection(searchText: searchText)
                } else if session.visibleConversations.isEmpty {
                    // Loading before empty: "All caught up" flashing on
                    // every open, while the first page was still in
                    // flight, announced an empty mailbox about a full
                    // one. The empty state is a *conclusion*, and it
                    // waits for the evidence.
                    if session.initialLoading {
                        ProgressView()
                    } else if session.searchQuery != nil {
                        ContentUnavailableView.search(text: searchText)
                    } else {
                        ContentUnavailableView(session.activeList.emptyMessage,
                                               systemImage: session.activeList.systemImage)
                    }
                } else {
                    List(session.visibleConversations, selection: $selection) { conversation in
                        NavigationLink {
                            ThreadView(conversation: conversation)
                        } label: {
                            ConversationRow(conversation: conversation,
                                            isSelecting: editMode == .active)
                        }
                        // A tint under the chosen rows. The system's own
                        // selection mark is a small circle at the left
                        // edge; on a dense list of mostly-grey rows it
                        // is not enough to see at a glance which ones
                        // the batch bar is about to act on.
                        .listRowBackground(
                            selection.contains(conversation.threadId)
                                ? Color.accentColor.opacity(0.18)
                                : Color.pageBackground
                        )
                        .contextMenu { ConversationRowMenu(conversation: conversation) }
                        .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                            // Archive owns the full swipe — the triage
                            // gesture in both benchmark apps, and it
                            // cannot destroy anything. Delete is behind
                            // it, keeps its confirmation, and
                            // deliberately lost the full-swipe slot: the
                            // fastest gesture in the app must not be the
                            // irreversible one.
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
                            // Read owns the right full swipe, as in
                            // Apple Mail; both directions are undone by
                            // the same swipe again.
                            Button {
                                Task { await session.toggleRead(conversation) }
                            } label: {
                                Label(
                                    ReadToggle.label(unread: conversation.unreadCount > 0),
                                    systemImage: ReadToggle.icon(unread: conversation.unreadCount > 0)
                                )
                            }
                            .tint(.blue)
                            // Reachable by swipe, not only by a long
                            // press. Reported from the phone as looking
                            // for it on the swipes and not finding it —
                            // which is the right place to look, and a
                            // menu nobody can see is a menu nobody has.
                            if !MailBucket.offered(from: session.activeList).isEmpty {
                                Button {
                                    pendingMove = conversation
                                } label: {
                                    Label("Move", systemImage: "tray.and.arrow.down")
                                }
                                .tint(.indigo)
                            }
                            Button {
                                Task { await session.toggleStarred(conversation) }
                            } label: {
                                Label(
                                    StarToggle.label(starred: conversation.flagged),
                                    systemImage: StarToggle.icon(starred: conversation.flagged)
                                )
                            }
                            .tint(.yellow)
                        }
                        .onAppear {
                            // Paging on the last row appearing, rather
                            // than on a "Load more" button: the row is
                            // already the thing that means "you have
                            // reached the bottom".
                            if conversation.threadId == session.visibleConversations.last?.threadId {
                                Task { await session.loadMore() }
                            }
                        }
                    }
                    .listStyle(.plain)
                    .softScrollEdges()
                    // Room for the search field iOS 26 pins to the
                    // bottom. It floats over the rows, so the last one
                    // sat under it and the row you scrolled to reach was
                    // the row you could not read.
                    .contentMargins(.bottom, 64, for: .scrollContent)
                    // On the List, not the Group: `refreshable` attaches
                    // to the nearest scrollable view, and a Group is not
                    // one.
                    .refreshable { await session.loadConversations() }
                }
            }
            // `.alert`, not `.confirmationDialog` — the same lesson this
            // file already carries for delete: the dialog renders as a
            // popover here and SwiftUI drops the `.cancel` button in
            // that presentation, so the sheet came up with no visible
            // way out but tapping past it.
            // An alert, not a `confirmationDialog`: on a phone that
            // one renders as a popover and drops what it is given —
            // twice in this app already. And a confirmation at all
            // because this cannot be undone and reaches rows the
            // reader cannot see.
            .alert(
                "Mark all as read?",
                isPresented: $confirmingMarkAllRead
            ) {
                Button("Mark all as read") {
                    Task { await session.markListRead() }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("Every conversation in \(session.activeList.title) will be marked read.")
            }
            .alert(
                "Move to",
                isPresented: Binding(
                    get: { pendingMove != nil },
                    set: { if !$0 { pendingMove = nil } }),
                presenting: pendingMove
            ) { conversation in
                ForEach(MailBucket.offered(from: session.activeList)) { bucket in
                    Button(bucket.label) {
                        Task { await session.move(conversation, to: bucket) }
                        pendingMove = nil
                    }
                }
                Button("Cancel", role: .cancel) { pendingMove = nil }
            }
            .navigationTitle(navigationTitle)
            // Inline, not large: the large title spends about fifty
            // points of every screen restating a word the toolbar has
            // room for, and this list is measured in rows.
            .inlineTitle()
            .overlay(alignment: .top) { FailureBanner() }
            .animation(.easeOut(duration: 0.2), value: session.banner)
            // The undo snackbar. Bottom-anchored but lifted above the
            // search field, which iOS 26 also puts at the bottom.
            .overlay(alignment: .bottom) {
                if session.pendingUndo != nil {
                    HStack(spacing: 12) {
                        Text(undoLabel)
                            .foregroundStyle(.white)
                        Button("Undo") {
                            Task { await session.undoArchive() }
                        }
                        .fontWeight(.semibold)
                        .accessibilityIdentifier("undo-archive")
                    }
                    .font(.subheadline)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    // Glass, because it floats over the list rather
                    // than belonging to it — and the rows staying
                    // legible through it is the point of the material.
                    .floatingGlass(in: .capsule)
                    .padding(.bottom, 72)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                }
            }
            .sheet(isPresented: $composing) { ComposeView() }
            .sheet(isPresented: $showingDrafts) { DraftsView() }
            .sheet(isPresented: $showingSettings) { SettingsView() }
            .searchable(text: $searchText, prompt: "Search mail")
            .onChange(of: searchText) { _, text in
                // Debounced, and the previous request cancelled: a
                // keystroke per character otherwise puts one search in
                // flight per letter, and the slowest one wins.
                searchTask?.cancel()
                searchTask = Task {
                    try? await Task.sleep(for: .milliseconds(250))
                    guard !Task.isCancelled else { return }
                    await session.search(text: text)
                }
            }
            // `.alert`, not `.confirmationDialog`. The dialog rendered as
            // a popover here and SwiftUI drops the `.cancel` button in
            // that presentation — leaving a destructive confirmation
            // whose only visible action was Delete, and tapping outside
            // as the undocumented way back. The safe way out of a
            // question about permanent deletion has to be on screen.
            .alert(
                "Delete conversation?",
                isPresented: Binding(
                    get: { pendingDelete != nil },
                    set: { if !$0 { pendingDelete = nil } }
                ),
                presenting: pendingDelete
            ) { conversation in
                Button("Delete", role: .destructive) {
                    Task { await session.delete(conversation) }
                    pendingDelete = nil
                }
                Button("Cancel", role: .cancel) { pendingDelete = nil }
            } message: { _ in
                Text("This will permanently delete all messages.")
            }
            .alert(
                "Delete \(pendingBatchDelete?.count ?? 0) conversations?",
                isPresented: Binding(
                    get: { pendingBatchDelete != nil },
                    set: { if !$0 { pendingBatchDelete = nil } }
                ),
                presenting: pendingBatchDelete
            ) { batch in
                Button("Delete", role: .destructive) {
                    leaveSelectMode()
                    Task { await session.deleteAll(batch) }
                    pendingBatchDelete = nil
                }
                Button("Cancel", role: .cancel) { pendingBatchDelete = nil }
            } message: { _ in
                Text("This will permanently delete all their messages.")
            }
            .toolbar {
                ToolbarItem(placement: .leadingAction) {
                    Menu {
                        Picker("List", selection: Binding(
                            get: { session.activeList },
                            set: { list in
                                // Selection belongs to the rows it was
                                // made over. Carried into another list
                                // it selects nothing visible and the
                                // batch bar acts on rows that are no
                                // longer on screen.
                                leaveSelectMode()
                                Task { await session.select(list) }
                            }
                        )) {
                            ForEach(MailList.allCases) { list in
                                Label(list.title, systemImage: list.systemImage).tag(list)
                            }
                        }
                        Divider()
                        // The whole mailbox, behind a confirmation: it
                        // is one tap, it cannot be undone, and the
                        // count it reports is the only evidence
                        // afterwards that it did anything.
                        Button {
                            confirmingMarkAllRead = true
                        } label: {
                            Label("Mark all as read", systemImage: "envelope.open")
                        }
                        .disabled(session.activeList == .send)
                        Button {
                            showingDrafts = true
                        } label: {
                            Label("Drafts", systemImage: "doc.text")
                        }
                        Divider()
                        Button {
                            showingSettings = true
                        } label: {
                            Label("Settings", systemImage: "gearshape")
                        }
                        // Named so a test reaches it by identity rather
                        // than by the word on it. The Japanese run of
                        // the screenshot lane could not find "Settings",
                        // because on that phone it says 設定.
                        .accessibilityIdentifier("open-settings")
                        // Sign-out moved off the bar when Select took
                        // its slot: triage is daily, sign-out is rare.
                        // It lives in Settings too, which is where
                        // someone looks for it first.
                        Button(role: .destructive) {
                            session.signOut()
                        } label: {
                            Label("Sign out", systemImage: "rectangle.portrait.and.arrow.right")
                        }
                    } label: {
                        Label("Lists", systemImage: "line.3.horizontal.decrease.circle")
                    }
                    .accessibilityIdentifier("open-lists")
                }
                ToolbarItem(placement: .primaryActions) {
                    Button {
                        composing = true
                    } label: {
                        Label("New message", systemImage: "square.and.pencil")
                    }
                    .accessibilityIdentifier("new-message")
                }
                if session.activeList != .send, !session.visibleConversations.isEmpty {
                    ToolbarItem(placement: .primaryActions) {
                        if editMode == .active {
                            Button("Done") {
                                withAnimation { editMode = .inactive }
                                selection.removeAll()
                            }
                        } else {
                            Button("Select") {
                                withAnimation { editMode = .active }
                            }
                        }
                    }
                }
                if editMode == .active {
                    ToolbarItem(placement: .leadingAction) {
                        Button(allSelected ? "None" : "All") {
                            withAnimation {
                                if allSelected {
                                    selection.removeAll()
                                } else {
                                    selection = Set(session.visibleConversations.map(\.threadId))
                                }
                            }
                        }
                        .accessibilityIdentifier("select-all")
                    }
                }
                // The batch bar. `bottomBar` keeps it clear of the
                // search field, which owns the very bottom on iOS 26.
                if editMode == .active {
                    ToolbarItemGroup(placement: .screenActions) {
                        Spacer()
                        Button("Read") {
                            Task { await session.markAllRead(selectedConversations) }
                            leaveSelectMode()
                        }
                        .disabled(selection.isEmpty)
                        Button("Archive") {
                            let batch = selectedConversations
                            leaveSelectMode()
                            Task { await session.archiveAll(batch) }
                        }
                        .disabled(selection.isEmpty)
                        Button("Delete", role: .destructive) {
                            pendingBatchDelete = selectedConversations
                        }
                        .disabled(selection.isEmpty)
                    }
                }
            }
            .environment(\.editMode, $editMode)
        }
    }

    /// While selecting, the title is the count — Apple Mail's pattern,
    /// and it spares the bottom bar a cramped pill.
    private var undoLabel: LocalizedStringKey {
        let count = session.pendingUndo?.rows.count ?? 1
        if count > 1 { return "Archived ×\(count)" }
        return "Archived"
    }

    private var navigationTitle: LocalizedStringKey {
        if editMode == .active { return "\(selection.count) selected" }
        return session.activeList.title
    }

    private var selectedConversations: [Wire.Conversation] {
        session.visibleConversations.filter { selection.contains($0.threadId) }
    }

    private func leaveSelectMode() {
        withAnimation { editMode = .inactive }
        selection.removeAll()
    }
}
