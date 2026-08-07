import SwiftUI

struct ConversationListView: View {
    @Environment(Session.self) private var session
    /// The thread a delete is waiting on confirmation for.
    @State private var pendingDelete: Wire.Conversation?
    @State private var searchText = ""
    @State private var searchTask: Task<Void, Never>?

    var body: some View {
        NavigationStack {
            // The empty state replaces the list rather than covering it.
            // `.overlay { if isEmpty { … } }` reads well and installs a
            // full-screen container either way — with rows present it is
            // an invisible sheet of glass over them, and every row
            // reports itself untappable underneath it.
            Group {
                if session.visibleConversations.isEmpty {
                    if session.searchQuery != nil {
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
                        .swipeActions(edge: .trailing) {
                            // Delete asks. `thread_actions.rs` unlinks the
                            // maildir files, so there is no trash and no
                            // undo to offer afterwards — the web client
                            // reached the same verb from a swipe without
                            // asking until 2026-08-05, and one gesture
                            // destroyed a thread outright.
                            Button(role: .destructive) {
                                pendingDelete = conversation
                            } label: {
                                Label("Delete", systemImage: "trash")
                            }
                        }
                        .swipeActions(edge: .leading) {
                            // Unread and star before archive: they are
                            // the two a thumb reaches for most, and both
                            // are undone by the same swipe again.
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
                            // Archive does not ask. It is reversible, and
                            // a question about a reversible action is
                            // noise that teaches people to dismiss
                            // questions.
                            Button {
                                Task { await session.archive(conversation) }
                            } label: {
                                Label("Archive", systemImage: "archivebox")
                            }
                            .tint(.green)
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
                    } label: {
                        Label("Lists", systemImage: "line.3.horizontal.decrease.circle")
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
        HStack(alignment: .top, spacing: 12) {
            // The unread marker is a dot rather than bold-everything:
            // one signal, in one place, that survives a long subject.
            // The dot carries the unread state, and a colour is not a
            // label — without this a VoiceOver user cannot tell a read
            // row from an unread one at all, and nothing in the row's
            // spoken text says which it is.
            Circle()
                .fill(conversation.unreadCount > 0 ? Color.accentColor : .clear)
                .frame(width: 8, height: 8)
                .padding(.top, 6)
                .accessibilityHidden(conversation.unreadCount == 0)
                .accessibilityLabel("Unread")

            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(sender)
                        .font(.subheadline.weight(conversation.unreadCount > 0 ? .semibold : .regular))
                        .lineLimit(1)
                    Spacer()
                    Text(Date(timeIntervalSince1970: TimeInterval(conversation.lastDate)),
                         format: .dateTime.month().day())
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                HStack(spacing: 4) {
                    Text(conversation.subject.isEmpty ? "(no subject)" : conversation.subject)
                        .font(.subheadline)
                        .lineLimit(1)
                    if conversation.flagged {
                        Image(systemName: "star.fill")
                            .font(.caption2)
                            .foregroundStyle(.yellow)
                            .accessibilityLabel("Starred")
                    }
                }
                Text(conversation.snippet)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 4)
    }
}
