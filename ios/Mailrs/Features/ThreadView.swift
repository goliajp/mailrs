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
                .softScrollEdges()
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

    /// Two keys rather than one with a plural rule: the catalog holds
    /// the singular and the count form separately, which is what lets
    /// a language that counts differently say so.
    private var countLabel: LocalizedStringKey {
        if messages.count == 1 { return "1 message" }
        return "\(messages.count) messages"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                ValueOrPlaceholder(value: conversation.subject, placeholder: "(no subject)")
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
            HStack(spacing: 4) {
                Text(countLabel)
                    .layoutPriority(1)
                if !participants.isEmpty {
                    Text(verbatim: "· \(participants)")
                        .lineLimit(1)
                }
            }
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
            RowDateText(epochSeconds: message.internalDate)
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
    @Environment(Session.self) private var session
    @State private var bodyHeight: CGFloat = 1

    /// Zero until the body has been measured — the card grows into its
    /// height rather than flashing a half-laid-out page.
    private var measuredOpacity: Double {
        if bodyHeight > 1 { return 1 }
        return 0
    }
    /// Per message, and not remembered: consenting to load one
    /// sender's images is not consent for the next one's.
    @State private var loadRemote = false

    private var files: [(index: Int, attachment: Wire.Attachment)] {
        MessageContent.listable(message.attachments, inlined: Array(inlineParts.keys.compactMap(indexOfPart)))
    }

    /// Content-ID → `data:` URI, for the parts the body points at.
    /// Fetched once per message; empty for the overwhelming majority of
    /// mail, which references nothing.
    @State private var inlineParts: [String: String] = [:]

    private func indexOfPart(_ contentId: String) -> Int? {
        message.attachments.firstIndex {
            InlineImages.normalise($0.contentId ?? "") == contentId
        }
    }

    /// The body as it will be drawn: the message's own pictures folded
    /// in, nothing fetched from the network.
    private func resolvedHTML(_ html: String) -> String {
        InlineImages.inline(html: html, parts: inlineParts)
    }

    private func loadInlineParts(_ html: String) async {
        let wanted = InlineImages.referenced(html: html, attachments: message.attachments)
        guard !wanted.isEmpty else { return }
        var found: [String: String] = [:]
        for index in wanted {
            let part = message.attachments[index]
            guard let raw = part.contentId else { continue }
            guard let data = try? await session.attachment(uid: message.uid, index: index) else { continue }
            found[InlineImages.normalise(raw)] = InlineImages.dataURI(
                contentType: part.contentType, data: data
            )
        }
        inlineParts = found
    }

    /// Two sentences, because "no content" and "no content but here are
    /// the files" are different situations for the reader.
    private var emptyBodyMessage: LocalizedStringKey {
        if files.isEmpty { return "This message has no readable content." }
        return "This message is its attachments."
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center, spacing: 8) {
                SenderAvatar(sender: message.sender, size: 32)
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 6) {
                        Text(SenderName.extractName(message.sender))
                            .font(.subheadline.weight(.semibold))
                            .lineLimit(1)
                            // The name yields before the date does: a
                            // truncated name is still a name, and a
                            // truncated timestamp is nothing.
                            .layoutPriority(1)
                        SenderTrustBadge(verdict: message.senderTrust)
                        Spacer(minLength: 4)
                        RowDateText(epochSeconds: message.internalDate, style: .stamp)
                    }
                    // Its own line, not a third thing competing for the
                    // name's: it is a whole sentence about where the
                    // message came from, and it is the line worth
                    // reading when it appears at all.
                    SenderClaimBadge(
                        actualDomain: SenderClaim.contradictedDomain(
                            displayName: SenderName.extractName(message.sender),
                            address: message.sender
                        )
                    )
                    .padding(.top, 1)
                    HStack(spacing: 6) {
                        Text("To: \(message.recipients)")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        AliasBadge(
                            alias: AliasMark.arrivedVia(
                                recipients: message.recipients,
                                myAddress: session.myAddress,
                                aliases: session.myAliases
                            )
                        )
                        // The recipient list yields first: it is
                        // already truncated and still readable, while a
                        // half-truncated address in the mark answers
                        // nothing at all.
                        .layoutPriority(1)
                    }
                }
            }
            .contentShape(Rectangle())
            .onTapGesture(perform: onHeaderTap)

            if !files.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(files, id: \.index) { file in
                        AttachmentRow(uid: message.uid, index: file.index, attachment: file.attachment)
                    }
                }
                .padding(.vertical, 4)
            }

            if let html = message.htmlBody, !html.isEmpty,
               !loadRemote, RemoteContent.hasRemoteReferences(html: html) {
                // Said plainly, and only when there is something to
                // say: fetching a remote image tells the sender the
                // message was opened, from where and when.
                Button {
                    withAnimation { loadRemote = true }
                } label: {
                    Label("Load images", systemImage: "photo")
                        .font(.caption.weight(.medium))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .floatingGlass(in: .capsule, tint: .accentColor)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("load-images")
            }

            switch MessageContent.body(html: message.htmlBody, text: message.textBody) {
            case .html(let html):
                // Faded in once measured rather than popping at full
                // size: until the height resolves the WebView is a
                // 1pt sliver, and revealing it mid-measure shows a
                // half-laid-out page.
                MessageBodyView(html: resolvedHTML(html), height: $bodyHeight,
                                blockRemote: !loadRemote)
                    .task(id: html) { await loadInlineParts(html) }
                    .frame(height: bodyHeight)
                    // Mail that keeps its own white paper would
                    // otherwise put square corners inside a rounded
                    // card.
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .opacity(measuredOpacity)
                    .animation(.easeIn(duration: 0.15), value: bodyHeight > 1)
            case .text(let text):
                Text(verbatim: text)
                    .font(.callout)
                    .textSelection(.enabled)
            case .empty:
                // A zip with nothing around it, a delivery report, a
                // signature with nothing this client can read: nine
                // messages in a 900-message sample of real mail have no
                // body, and they used to open as a blank card.
                Label(emptyBodyMessage, systemImage: "doc.questionmark")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
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
            // A mark, not a sentence. "Unverified sender" spelled out
            // beside a name and a date is two words too many for the
            // line, and the header wrapped — which reads as a defect
            // whatever it says. Colour and shape carry it; the words
            // stay as the accessibility label, where they are read
            // aloud rather than competing for width.
            Image(systemName: "exclamationmark.shield.fill")
                .font(.footnote)
                .foregroundStyle(.orange)
                .accessibilityLabel("Unverified sender")
        case "verified":
            Image(systemName: "checkmark.seal.fill")
                .font(.footnote)
                .foregroundStyle(.green)
                .accessibilityLabel("Verified sender")
        default:
            EmptyView()
        }
    }
}

/// Which of my addresses this arrived at, when it was not the obvious
/// one.
///
/// Mail to `sales@` and mail to `lihao@` land in the same mailbox and
/// looked identical once they got there. The address a message was sent
/// to is part of what it is: it decides whether to answer as a person
/// or as a desk, and an address only one service was ever given makes
/// mail arriving at it suspect on its own.
///
/// Same symbol as the Aliases screen, so the mark and the place you
/// manage it are recognisably the same subject.
struct AliasBadge: View {
    let alias: String?

    var body: some View {
        if let alias {
            HStack(spacing: 3) {
                Image(systemName: "arrow.triangle.branch")
                    .font(.system(size: 9, weight: .semibold))
                Text(verbatim: alias)
                    .font(.caption2)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .foregroundStyle(Color.accentColor)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Color.accentColor.opacity(0.12), in: Capsule())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Delivered to \(alias)")
        }
    }
}

/// Where the message actually came from, when its sender's name says
/// somewhere else.
///
/// The thread header shows a display name and never an address, which
/// is precisely the gap brand impersonation lives in: `Amazon.co.jp`
/// reads as Amazon whether it was sent by Amazon or by
/// `mail07.jqjintaiyang.com`. Measured on this mailbox — 1,500 From
/// headers, 206 names containing a domain, **8** disagreeing with the
/// domain that sent them, six of those unmistakable — so it is rare
/// enough to be worth interrupting for.
///
/// It states rather than accuses. "This came from X" is useful even
/// when X turns out to be the same company's second domain, which is
/// what makes it safe to show on a signal that cannot be perfect.
struct SenderClaimBadge: View {
    let actualDomain: String?

    var body: some View {
        if let actualDomain {
            HStack(spacing: 3) {
                Image(systemName: "questionmark.circle.fill")
                    .font(.system(size: 9, weight: .semibold))
                Text(verbatim: actualDomain)
                    .font(.caption2)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .foregroundStyle(.orange)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Color.orange.opacity(0.14), in: Capsule())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Sent from \(actualDomain)")
        }
    }
}
