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
            index > 0 ? rows[index - 1] : nil,
            index + 1 < rows.count ? rows[index + 1] : nil
        )
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
                                MessageCard(message: message) {
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
                .background(Color(.systemGroupedBackground))
            }
        }
        // Empty on purpose: the subject is the header's job now, where
        // it can wrap and be read. A nav-bar copy squeezed between the
        // back button and three toolbar buttons showed six words and an
        // ellipsis, and was the only place the subject appeared at all.
        .navigationTitle("")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            // Apple Mail's chevrons: process the mailbox serially
            // without bouncing back to the list between messages. Each
            // step is an open, so it marks read through the same rule
            // as any open.
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    step(to: neighbours.previous)
                } label: {
                    Label("Previous thread", systemImage: "chevron.up")
                }
                .disabled(neighbours.previous == nil)
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    step(to: neighbours.next)
                } label: {
                    Label("Next thread", systemImage: "chevron.down")
                }
                .disabled(neighbours.next == nil)
            }
            ToolbarItem(placement: .topBarTrailing) {
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
            ToolbarItemGroup(placement: .bottomBar) {
                Button {
                    Task { await session.toggleStarred(starTarget) }
                } label: {
                    Label(
                        isStarred ? "Unstar" : "Star",
                        systemImage: isStarred ? "star.fill" : "star"
                    )
                }
                .tint(isStarred ? .yellow : nil)
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

/// The thread's own title block.
///
/// Apple Mail gives the subject a line of its own above the messages;
/// a nav bar cannot, and a subject nobody can read is a mail app's
/// worst small failure.
private struct ThreadHeader: View {
    let conversation: Wire.Conversation
    let messages: [Wire.Message]
    let myAddress: String

    /// Everyone who wrote in this thread, in the order they first
    /// appear, minus me — the same rule the row's face follows.
    private var participants: String {
        var seen = Set<String>()
        var names: [String] = []
        for message in messages {
            let email = SenderName.extractEmail(message.sender)
            if email == myAddress || seen.contains(email) { continue }
            seen.insert(email)
            names.append(SenderName.extractName(message.sender))
        }
        return names.joined(separator: ", ")
    }

    private var countLabel: String {
        messages.count == 1 ? "1 message" : "\(messages.count) messages"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(conversation.subject.isEmpty ? "(no subject)" : conversation.subject)
                    .font(.title3.weight(.semibold))
                    // Named so a test can ask this element what it says
                    // rather than asking the screen whether a string is
                    // anywhere on it — the list behind a push can still
                    // answer yes.
                    .accessibilityIdentifier("thread-subject")
                    // Three lines: enough for the long ones mail
                    // actually carries, bounded so the messages are
                    // still on screen when the thread opens.
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
                if conversation.flagged {
                    Image(systemName: "star.fill")
                        .font(.caption)
                        .foregroundStyle(.yellow)
                        .accessibilityLabel("Starred")
                }
            }
            Text(participants.isEmpty ? countLabel : "\(countLabel) · \(participants)")
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 12)
        .padding(.top, 4)
        .padding(.bottom, 2)
    }
}

/// One message folded to a line: who, a breath of what, when. Tap for
/// the full card.
private struct CollapsedMessageRow: View {
    let message: Wire.Message

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(SenderName.extractName(message.sender))
                .font(.subheadline)
                .lineLimit(1)
                .layoutPriority(1)
            Text(ThreadCollapse.snippet(message.textBody))
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Spacer(minLength: 8)
            if !message.attachments.isEmpty {
                Image(systemName: "paperclip")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel("Has attachments")
            }
            Text(Date(timeIntervalSince1970: TimeInterval(message.internalDate)),
                 format: .dateTime.month().day())
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(Color(.secondarySystemGroupedBackground).opacity(0.6),
                    in: RoundedRectangle(cornerRadius: 12))
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("collapsed-\(message.uid)")
        .accessibilityAddTraits(.isButton)
        .accessibilityHint("Expands the message")
    }
}

private struct MessageCard: View {
    let message: Wire.Message
    /// Tapping the header folds the card back to its line.
    let onHeaderTap: () -> Void
    @State private var bodyHeight: CGFloat = 1

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center, spacing: 8) {
                SenderAvatar(sender: message.sender, size: 32)
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 6) {
                        Text(SenderName.extractName(message.sender))
                            .font(.subheadline.weight(.semibold))
                            .lineLimit(1)
                        SenderTrustBadge(verdict: message.senderTrust)
                        Spacer()
                        Text(Date(timeIntervalSince1970: TimeInterval(message.internalDate)),
                             format: .dateTime.month().day().hour().minute())
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Text("To: \(message.recipients)")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture(perform: onHeaderTap)

            if !message.attachments.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(Array(message.attachments.enumerated()), id: \.offset) { index, attachment in
                        AttachmentRow(uid: message.uid, index: index, attachment: attachment)
                    }
                }
                .padding(.vertical, 4)
            }

            if let html = message.htmlBody, !html.isEmpty {
                // Faded in once measured rather than popping at full
                // size: until the height resolves the WebView is a
                // 1pt sliver, and revealing it mid-measure shows a
                // half-laid-out page.
                MessageBodyView(html: html, height: $bodyHeight)
                    .frame(height: bodyHeight)
                    // Mail that keeps its own white paper would
                    // otherwise put square corners inside a rounded
                    // card.
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .opacity(bodyHeight > 1 ? 1 : 0)
                    .animation(.easeIn(duration: 0.15), value: bodyHeight > 1)
            } else {
                Text(message.textBody ?? "")
                    .font(.callout)
                    .textSelection(.enabled)
            }
        }
        .padding(12)
        .background(Color(.secondarySystemGroupedBackground),
                    in: RoundedRectangle(cornerRadius: 12))
    }
}

/// What the server's cryptographic checks concluded about the sender.
///
/// Only `suspicious` is loud. A verified sender is the ordinary case and
/// a badge on every message trains people to stop reading badges; the
/// one worth interrupting for is mail whose From does not survive DMARC,
/// because that is the shape of a forgery. `unverified` and mail that
/// predates the signal say nothing rather than implying safety.
private struct SenderTrustBadge: View {
    let verdict: String

    var body: some View {
        switch verdict {
        case "suspicious":
            Label("Unverified sender", systemImage: "exclamationmark.shield.fill")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.orange)
                .labelStyle(.titleAndIcon)
        case "verified":
            Image(systemName: "checkmark.seal.fill")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .accessibilityLabel("Verified sender")
        default:
            EmptyView()
        }
    }
}
