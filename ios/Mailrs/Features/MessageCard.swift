import SwiftUI

/// One message: its header, its body, and the row it folds back into.
///
/// Split out of `ThreadView.swift` at the 500-line limit. `private` came
/// off on the way: in Swift that is file scope, and these are drawn from
/// a file that is no longer this one.

struct MessageCard: View {
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


/// One message folded to a line: who, a breath of what, when. Tap for
/// the full card.
struct CollapsedMessageRow: View {
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
