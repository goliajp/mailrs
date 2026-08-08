import SwiftUI

struct ConversationListView: View {
    @Environment(Session.self) private var session
    /// The thread a delete is waiting on confirmation for.
    @State private var pendingDelete: Wire.Conversation?
    @State private var searchText = ""
    @State private var searchTask: Task<Void, Never>?
    @State private var composing = false
    @State private var showingDrafts = false
    /// Multi-select state. `selection` only means anything while
    /// `editMode` is active; leaving select mode clears it.
    @State private var showingSettings = false
    @State private var selection = Set<String>()
    @State private var editMode: EditMode = .inactive
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
                            ConversationRow(conversation: conversation)
                        }
                        .contextMenu {
                            // Junk lives in the long-press menu, not the
                            // swipe rows — those are full, and a verdict
                            // that trains the filter deserves a deliberate
                            // gesture rather than the one you make fifty
                            // times a day.
                            if session.activeList == .junk {
                                Button {
                                    Task { await session.setJunk(conversation, junk: false) }
                                } label: {
                                    Label("Not junk", systemImage: "checkmark.shield")
                                }
                            } else {
                                Button(role: .destructive) {
                                    Task { await session.setJunk(conversation, junk: true) }
                                } label: {
                                    Label("Mark as junk", systemImage: "xmark.bin")
                                }
                            }
                        }
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
                                    conversation.unreadCount > 0 ? "Read" : "Unread",
                                    systemImage: conversation.unreadCount > 0
                                        ? "envelope.open" : "envelope.badge"
                                )
                            }
                            .tint(.blue)
                            Button {
                                Task { await session.toggleStarred(conversation) }
                            } label: {
                                Label(
                                    conversation.flagged ? "Unstar" : "Star",
                                    systemImage: conversation.flagged ? "star.slash" : "star"
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
                    // On the List, not the Group: `refreshable` attaches
                    // to the nearest scrollable view, and a Group is not
                    // one.
                    .refreshable { await session.loadConversations() }
                }
            }
            .navigationTitle(navigationTitle)
            // Inline, not large: the large title spends about fifty
            // points of every screen restating a word the toolbar has
            // room for, and this list is measured in rows.
            .navigationBarTitleDisplayMode(.inline)
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
                ToolbarItem(placement: .topBarLeading) {
                    Menu {
                        Picker("List", selection: Binding(
                            get: { session.activeList },
                            set: { list in Task { await session.select(list) } }
                        )) {
                            ForEach(MailList.allCases) { list in
                                Label(list.title, systemImage: list.systemImage).tag(list)
                            }
                        }
                        Divider()
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
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        composing = true
                    } label: {
                        Label("New message", systemImage: "square.and.pencil")
                    }
                }
                if session.activeList != .send, !session.visibleConversations.isEmpty {
                    ToolbarItem(placement: .topBarTrailing) {
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
                // The batch bar. `bottomBar` keeps it clear of the
                // search field, which owns the very bottom on iOS 26.
                if editMode == .active {
                    ToolbarItemGroup(placement: .bottomBar) {
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

struct ConversationRow: View {
    let conversation: Wire.Conversation
    @Environment(Session.self) private var session
    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone
    @Environment(\.locale) private var locale

    /// The environment's calendar carries the language but not the
    /// chosen zone — they are separate keys, and a date read in the
    /// phone's zone under a chosen one is the bug this avoids.
    private var readerCalendar: Calendar {
        var calendar = calendar
        calendar.timeZone = timeZone
        calendar.locale = locale
        return calendar
    }

    private var sender: String {
        SenderName.rowFace(
            participants: conversation.participants,
            myAddress: session.myAddress
        )
    }

    /// Whose color and initial the avatar wears — the same participant
    /// the name comes from.
    private var face: String {
        let mine = session.myAddress
        let others = conversation.participants.filter { SenderName.extractEmail($0) != mine }
        return others.first ?? conversation.participants.first ?? ""
    }

    private var extraParticipants: Int {
        let mine = session.myAddress
        let others = conversation.participants.filter { SenderName.extractEmail($0) != mine }
        return max(0, others.count - 1)
    }

    /// The web row's count chip: received and sent split when the
    /// thread has both directions, a plain total otherwise.
    private var countLabel: String? {
        guard conversation.messageCount > 1 else { return nil }
        if conversation.sentCount > 0, conversation.receivedCount > 0 {
            return "\(conversation.receivedCount)↓ \(conversation.sentCount)↑"
        }
        return "×\(conversation.messageCount)"
    }

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            // The avatar wears the unread state as a badge on its rim —
            // the dot keeps its VoiceOver label ("Unread"), because a
            // colour is not a label.
            SenderAvatar(sender: face)
                .overlay(alignment: .topTrailing) {
                    if conversation.unreadCount > 0 {
                        Circle()
                            .fill(Color.accentColor)
                            .frame(width: 11, height: 11)
                            .overlay(Circle().stroke(Color(.systemBackground), lineWidth: 2))
                            .offset(x: 1, y: -1)
                            .accessibilityHidden(false)
                            .accessibilityLabel("Unread")
                    }
                }

            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 6) {
                    Text(sender)
                        .font(.subheadline.weight(conversation.unreadCount > 0 ? .semibold : .regular))
                        .lineLimit(1)
                    if extraParticipants > 0 {
                        Text("+\(extraParticipants)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    if conversation.importanceLevel == "critical"
                        || conversation.importanceLevel == "important" {
                        Image(systemName: "exclamationmark.circle.fill")
                            .font(.caption2)
                            .foregroundStyle(
                                conversation.importanceLevel == "critical" ? .red : .orange
                            )
                            .accessibilityLabel(
                                conversation.importanceLevel == "critical"
                                    ? "Critical" : "Important"
                            )
                    }
                    Spacer(minLength: 4)
                    if let countLabel {
                        Text(countLabel)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                            .padding(.horizontal, 4)
                            .padding(.vertical, 1)
                            .background(Color(.tertiarySystemFill), in: Capsule())
                    }
                    Text(RowDate.label(epochSeconds: conversation.lastDate,
                                       calendar: readerCalendar))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                HStack(spacing: 4) {
                    Text(conversation.subject.isEmpty ? "(no subject)" : conversation.subject)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    if conversation.flagged {
                        Image(systemName: "star.fill")
                            .font(.caption2)
                            .foregroundStyle(.yellow)
                            .accessibilityLabel("Starred")
                    }
                    Spacer(minLength: 0)
                }
            }
        }
        .padding(.vertical, 2)
    }
}
