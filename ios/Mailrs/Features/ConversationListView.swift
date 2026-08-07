import SwiftUI

struct ConversationListView: View {
    @Environment(Session.self) private var session
    /// The thread a delete is waiting on confirmation for.
    @State private var pendingDelete: Wire.Conversation?
    @State private var searchText = ""
    @State private var searchTask: Task<Void, Never>?
    @State private var composing = false
    @State private var showingDrafts = false

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
                    List(session.visibleConversations) { conversation in
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
                    // On the List, not the Group: `refreshable` attaches
                    // to the nearest scrollable view, and a Group is not
                    // one.
                    .refreshable { await session.loadConversations() }
                }
            }
            .navigationTitle(session.activeList.title)
            .sheet(isPresented: $composing) { ComposeView() }
            .sheet(isPresented: $showingDrafts) { DraftsView() }
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
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Sign out") { session.signOut() }
                }
            }
        }
    }
}

struct ConversationRow: View {
    let conversation: Wire.Conversation

    private var sender: String {
        conversation.participants.first ?? "(unknown)"
    }

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            // The dot carries the unread state, and a colour is not a
            // label — without this a VoiceOver user cannot tell a read
            // row from an unread one at all.
            Circle()
                .fill(conversation.unreadCount > 0 ? Color.accentColor : .clear)
                .frame(width: 8, height: 8)
                .accessibilityHidden(conversation.unreadCount == 0)
                .accessibilityLabel("Unread")

            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 6) {
                    Text(sender)
                        .font(.subheadline.weight(conversation.unreadCount > 0 ? .semibold : .regular))
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    if conversation.messageCount > 1 {
                        Text("×\(conversation.messageCount)")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                    }
                    Text(RowDate.label(epochSeconds: conversation.lastDate))
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
