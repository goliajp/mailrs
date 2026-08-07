import SwiftUI

struct ThreadView: View {
    /// Mutable: the ▲▼ chevrons walk the list without leaving the
    /// screen, so which conversation this shows changes in place.
    @State private var conversation: Wire.Conversation
    @Environment(Session.self) private var session
    @State private var messages: [Wire.Message] = []
    @State private var failure: String?
    @State private var replying = false
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
                    LazyVStack(alignment: .leading, spacing: 0) {
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
                            Divider()
                        }
                    }
                }
            }
        }
        .navigationTitle(conversation.subject.isEmpty ? "(no subject)" : conversation.subject)
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
        }
        .sheet(isPresented: $replying) {
            ReplyView(thread: conversation, replyingTo: messages.last)
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
            HStack(alignment: .firstTextBaseline) {
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
            .contentShape(Rectangle())
            .onTapGesture(perform: onHeaderTap)
            Text("To: \(message.recipients)")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)

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
                    .opacity(bodyHeight > 1 ? 1 : 0)
                    .animation(.easeIn(duration: 0.15), value: bodyHeight > 1)
            } else {
                Text(message.textBody ?? "")
                    .font(.callout)
                    .textSelection(.enabled)
            }
        }
        .padding(12)
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
