import SwiftUI

import SwiftUI

struct ThreadView: View {
    /// Mutable: the ▲▼ chevrons walk the list without leaving the
    /// screen, so which conversation this shows changes in place.
    @State private var conversation: Wire.Conversation
    @Environment(Session.self) private var session
    @State private var messages: [Wire.Message] = []
    @State private var failure: String?
    @State private var replying = false
    @State private var confirmingDelete = false
    @Environment(\.dismiss) private var dismiss
    /// Messages whose fold state the reader explicitly flipped. What is
    /// actually expanded is derived — (is last) XOR (in here) — so a
    /// thread change only has to clear this set.
    @State private var toggled = Set<UInt32>()

    init(conversation: Wire.Conversation) {
        _conversation = State(initialValue: conversation)
    }

    /// Neighbours in the order the list draws — the one ordering the
    /// person was just looking at, whatever list and search produced it.
    private var neighbours: (previous: Wire.Conversation?, next: Wire.Conversation?) {
        let rows = session.visibleConversations
        guard let index = rows.firstIndex(where: { $0.threadId == conversation.threadId }) else {
            return (nil, nil)
        }
        return (
            rows[safe: index - 1],
            rows[safe: index + 1]
        )
    }

    private var starLabel: LocalizedStringKey {
        if isStarred { return "Unstar" }
        return "Star"
    }

    private var starIcon: String {
        if isStarred { return "star.fill" }
        return "star"
    }

    private var starTint: Color? {
        if isStarred { return .yellow }
        return nil
    }

    /// The star's state, derived from the list rather than mirrored
    /// here. A local copy toggled alongside the session's meant the
    /// button's decision depended on which of the two ran first — and
    /// `Task { }` defers, so the session read the value this screen
    /// had already flipped and sent the opposite verb.
    private var isStarred: Bool {
        starTarget.flagged
    }

    /// The list's row while it is listed, this screen's snapshot
    /// otherwise — the row leaves the list on archive, and the thread
    /// can still be on screen for the moment before it dismisses.
    private var starTarget: Wire.Conversation {
        session.visibleConversations.first { $0.threadId == conversation.threadId }
            ?? conversation
    }

    private func step(to target: Wire.Conversation?) {
        guard let target else { return }
        messages = []
        failure = nil
        toggled = []
        conversation = target
    }

    var body: some View {
        Group {
            // Same reason as the inbox: an overlay over a scroll view is
            // a pane of glass over everything inside it.
            if let failure {
                ContentUnavailableView("Could not open this thread",
                                       systemImage: "exclamationmark.triangle",
                                       description: Text(failure))
            } else if messages.isEmpty {
                ProgressView()
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ThreadHeader(
                            conversation: conversation,
                            messages: messages,
                            myAddress: session.myAddress
                        )
                        ForEach(messages) { message in
                            if ThreadCollapse.isExpanded(
                                uid: message.uid,
                                lastUid: messages.last?.uid,
                                toggled: toggled
                            ) {
                                // The header folds the card; the body
                                // keeps its own taps (links, text
                                // selection, attachments).
                                MessageCard(message: message, threadId: conversation.threadId) {
                                    withAnimation { toggled.formSymmetricDifference([message.uid]) }
                                }
                            } else {
                                CollapsedMessageRow(message: message)
                                    .onTapGesture {
                                        withAnimation { toggled.formSymmetricDifference([message.uid]) }
                                    }
                            }
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                }
                .softScrollEdges()
                // Room for the floating bar. It hovers over the scroll
                // view rather than taking space from it, so without this
                // the last message's final lines sit under the star and
                // the bin permanently — reported from the phone as the
                // bottom of some mail being covered. `contentMargins`
                // adds it to the scrolled content only, so the glass
                // still has content passing beneath it.
                .contentMargins(.bottom, 64, for: .scrollContent)
                .background(Color.groupedBackground)
            }
        }
        // Empty on purpose: the subject is the header's job now, where
        // it can wrap and be read. A nav-bar copy squeezed between the
        // back button and three toolbar buttons showed six words and an
        // ellipsis, and was the only place the subject appeared at all.
        .navigationTitle("")
        .inlineTitle()
        .toolbar {
            // Apple Mail's chevrons: process the mailbox serially
            // without bouncing back to the list between messages. Each
            // step is an open, so it marks read through the same rule
            // as any open.
            ToolbarItem(placement: .primaryActions) {
                Button {
                    step(to: neighbours.previous)
                } label: {
                    Label("Previous thread", systemImage: "chevron.up")
                }
                .disabled(neighbours.previous == nil)
            }
            ToolbarItem(placement: .primaryActions) {
                Button {
                    step(to: neighbours.next)
                } label: {
                    Label("Next thread", systemImage: "chevron.down")
                }
                .disabled(neighbours.next == nil)
            }
            ToolbarItem(placement: .primaryActions) {
                Button {
                    replying = true
                } label: {
                    Label("Reply", systemImage: "arrowshape.turn.up.left")
                }
                .disabled(messages.isEmpty)
            }
            // Triage from inside the thread — Apple Mail's bottom bar.
            // Without it every verdict costs a trip back to the list and
            // a swipe on a row you have to find again.
            ToolbarItemGroup(placement: .screenActions) {
                Button {
                    Task { await session.toggleStarred(starTarget) }
                } label: {
                    Label(starLabel, systemImage: starIcon)
                }
                .tint(starTint)
                Spacer()
                Button {
                    Task { await session.archive(conversation) }
                    // Leaves immediately: the row is already gone from
                    // the list behind, and the undo toast is waiting
                    // there. Staying would leave a thread on screen
                    // that the mailbox no longer lists.
                    dismiss()
                } label: {
                    Label("Archive", systemImage: "archivebox")
                }
                Spacer()
                Button(role: .destructive) {
                    confirmingDelete = true
                } label: {
                    Label("Delete", systemImage: "trash")
                }
                Spacer()
                // The verdicts you reach only after reading: that this
                // can wait, and that it should not have come at all.
                Menu {
                    Button {
                        Task { await session.markUnread(conversation) }
                        // Leaving is the point — the thread was opened
                        // to be dealt with later, and staying inside a
                        // message marked unread is a contradiction on
                        // screen.
                        dismiss()
                    } label: {
                        Label("Mark as unread", systemImage: "envelope.badge")
                    }
                    if session.activeList == .junk {
                        Button {
                            Task { await session.setJunk(conversation, junk: false) }
                            dismiss()
                        } label: {
                            Label("Not junk", systemImage: "checkmark.shield")
                        }
                    } else {
                        Button(role: .destructive) {
                            Task { await session.setJunk(conversation, junk: true) }
                            dismiss()
                        } label: {
                            Label("Mark as junk", systemImage: "xmark.bin")
                        }
                    }
                } label: {
                    Label("More", systemImage: "ellipsis")
                }
            }
        }
        .sheet(isPresented: $replying) {
            ReplyView(thread: conversation, replyingTo: messages.last)
        }
        .alert("Delete conversation?", isPresented: $confirmingDelete) {
            Button("Delete", role: .destructive) {
                Task {
                    // Deletion is not optimistic anywhere: the server
                    // unlinks the files, so the screen leaves only once
                    // it says they are gone.
                    await session.delete(conversation)
                    dismiss()
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This will permanently delete all messages.")
        }

        .task(id: conversation.threadId) {
            // Last fetch first: an opened conversation reads from disk
            // while — or without — the network answering.
            if messages.isEmpty,
               let cached = session.cachedMessages(threadId: conversation.threadId) {
                messages = cached
            }
            do {
                let fresh = try await session.messages(threadId: conversation.threadId)
                // Identical answers skip the swap: replacing the array
                // rebuilds every card and re-measures every body for
                // nothing.
                if fresh != messages { messages = fresh }
                // After the messages are on screen, not before: an open
                // that failed to load anything has not been read.
                await session.markThreadRead(conversation)
            } catch {
                // Cached mail on screen is a readable thread; the error
                // pane would replace it with an apology. It also stays
                // unread — showing yesterday's copy is not an open that
                // reached the server.
                if messages.isEmpty {
                    failure = error.localizedDescription
                }
            }
        }
    }
}
