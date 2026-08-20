import SwiftUI

/// One message: its header, its body, and the row it folds back into.
///
/// Split out of `ThreadView.swift` at the 500-line limit. `private` came
/// off on the way: in Swift that is file scope, and these are drawn from
/// a file that is no longer this one.

struct MessageCard: View {
    let message: Wire.Message
    /// The thread the message is in — the unsubscribe call names both,
    /// because the server looks the message up rather than trusting a
    /// URL from the client.
    let threadId: String
    /// Tapping the header folds the card back to its line.
    let onHeaderTap: () -> Void
    @Environment(Session.self) private var session
    @Environment(\.theme) private var theme
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var bodyHeight: CGFloat = 1
    @State private var showingSource = false
    @State private var showingQuoted = false

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
        // Beacons out first, whatever the reader has consented to:
        // "Load images" is a decision about pictures, not about being
        // counted. 71% of real HTML mail carries at least one.
        InlineImages.inline(html: TrackingPixels.strip(html: html), parts: inlineParts)
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

    /// Who it is from, whether they are who they say, and when.
    ///
    /// Stacked at the accessibility sizes for the reason the list row
    /// is (`RowLayout`): side by side, the name loses to a timestamp.
    /// What a reader can do with the person who sent this.
    ///
    /// A long press on the name, because the name is what they are
    /// looking at when the question occurs to them — "who is this",
    /// "what else have they sent me", "make this stop". Every answer
    /// here is built from something the app already has: the search
    /// this client already runs, and the sender lists the server has
    /// always kept and no client offered until this week.
    @ViewBuilder
    private var senderActions: some View {
        let address = SenderName.extractEmail(message.sender)
        if !address.isEmpty {
            Button {
                Task { await session.search(text: address) }
            } label: {
                Label("Search this sender", systemImage: "magnifyingglass")
            }
            Button {
                UIPasteboard.general.string = address
            } label: {
                Label("Copy address", systemImage: "doc.on.doc")
            }
            Divider()
            Button {
                Task { await session.addSender(address, to: .blocked) }
            } label: {
                Label("Always block", systemImage: "hand.raised")
            }
            Button {
                Task { await session.addSender(address, to: .allowed) }
            } label: {
                Label("Always allow", systemImage: "checkmark.shield")
            }
        }
    }

    @ViewBuilder
    private var senderLine: some View {
        if RowLayout.stacksHeader(typeSize) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(SenderName.extractName(message.sender))
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(RowLayout.senderLines(typeSize))
                    SenderTrustBadge(verdict: message.senderTrust)
                }
                RowDateText(epochSeconds: message.internalDate, style: .stamp)
            }
            .contextMenu { senderActions }
        } else {
            HStack(spacing: 6) {
                Text(SenderName.extractName(message.sender))
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                    // The name yields before the date does: a truncated
                    // name is still a name, and a truncated timestamp is
                    // nothing.
                    .layoutPriority(1)
                SenderTrustBadge(verdict: message.senderTrust)
                Spacer(minLength: 4)
                RowDateText(epochSeconds: message.internalDate, style: .stamp)
            }
            .contextMenu { senderActions }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: RowLayout.gutterAlignment(typeSize), spacing: 8) {
                SenderAvatar(sender: message.sender, size: 32)
                VStack(alignment: .leading, spacing: 1) {
                    senderLine
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
                        // Middle truncation keeps both ends of one
                        // address readable, which is the right trade on
                        // one line. At the accessibility sizes one line
                        // holds about six characters and the result was
                        // "me…ple.com>" — so there, it wraps instead.
                        Text("To: \(message.recipients)")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(RowLayout.recipientLines(typeSize))
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

            // Above the body, because the meeting is what the message
            // is about and the HTML around it is packaging.
            // One view for both: an invitation when the message
            // carries a calendar part, and the dates written in the
            // prose when it does not — which is most mail about a
            // meeting.
            InviteCard(uid: message.uid, method: message.inviteMethod)
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
                let split = QuotedHistory.split(text)
                // Not `Text(verbatim:)`: 70% of real plain-text mail has
                // a URL in it, and that spelling renders every one of
                // them as characters you can read and not follow.
                Text(PlainTextLinks.attributed(split.body))
                    .font(.callout)
                    .textSelection(.enabled)
                    .tint(theme.accent)
                if let quoted = split.quoted {
                    // Folded, never dropped. Where a reply carries
                    // history at all it is a median 81% of the body,
                    // and the reader has read it once already.
                    Button {
                        withAnimation { showingQuoted.toggle() }
                    } label: {
                        Label(
                            showingQuoted ? "Hide quoted text" : "Show quoted text",
                            systemImage: showingQuoted ? "chevron.up" : "ellipsis")
                            .font(.caption)
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.secondary)
                    if showingQuoted {
                        Text(PlainTextLinks.attributed(quoted))
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                            .tint(theme.accent)
                    }
                }
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

            UnsubscribeFooter(threadId: threadId, message: message)
        }
        .padding(12)
        .background(Color(.secondarySystemGroupedBackground),
                    in: RoundedRectangle(cornerRadius: 12))
        // Long press, which is where every other iOS app keeps the
        // things you do to one item. The thread's own bar has reply,
        // archive and the junk verdict — those act on the whole
        // conversation. These three act on *this message*, and none of
        // them had anywhere to live.
        .contextMenu {
            Button {
                UIPasteboard.general.string = MessageActions.plainText(message)
            } label: {
                Label("Copy text", systemImage: "doc.on.doc")
            }
            ShareLink(item: MessageActions.shareable(message)) {
                Label("Share", systemImage: "square.and.arrow.up")
            }
            Divider()
            Button {
                showingSource = true
            } label: {
                Label("View source", systemImage: "chevron.left.forwardslash.chevron.right")
            }
        }
        .sheet(isPresented: $showingSource) {
            MessageSourceSheet(uid: message.uid)
        }
    }
}


/// One message folded to a line: who, a breath of what, when. Tap for
/// the full card.
struct CollapsedMessageRow: View {
    let message: Wire.Message
    @Environment(\.dynamicTypeSize) private var typeSize

    /// Who, a breath of what, when.
    ///
    /// One line while they fit. At the accessibility sizes they do not:
    /// the name came out as "Alice…" and the snippet as a single
    /// character, with the paperclip and the date taking the rest — a
    /// folded row that says nothing is worse than no folded row.
    @ViewBuilder
    private var layout: some View {
        if RowLayout.stacksHeader(typeSize) {
            VStack(alignment: .leading, spacing: 2) {
                Text(SenderName.extractName(message.sender))
                    .font(.subheadline)
                    .lineLimit(RowLayout.senderLines(typeSize))
                Text(ThreadCollapse.snippet(message.textBody))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                HStack(spacing: 6) {
                    paperclip
                    RowDateText(epochSeconds: message.internalDate)
                    Spacer(minLength: 0)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        } else {
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
                paperclip
                RowDateText(epochSeconds: message.internalDate)
            }
        }
    }

    @ViewBuilder
    private var paperclip: some View {
        if !message.attachments.isEmpty {
            Image(systemName: "paperclip")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .accessibilityLabel("Has attachments")
        }
    }

    var body: some View {
        layout
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
