import SwiftUI

struct ThreadView: View {
    let conversation: Wire.Conversation
    @Environment(Session.self) private var session
    @State private var messages: [Wire.Message] = []
    @State private var failure: String?
    @State private var replying = false

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
                            MessageCard(message: message)
                            Divider()
                        }
                    }
                }
            }
        }
        .navigationTitle(conversation.subject.isEmpty ? "(no subject)" : conversation.subject)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
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

        .task {
            do {
                messages = try await session.messages(threadId: conversation.threadId)
            } catch {
                failure = error.localizedDescription
            }
        }
    }
}

private struct MessageCard: View {
    let message: Wire.Message
    @State private var bodyHeight: CGFloat = 1

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text(message.sender)
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
                .font(.caption)
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
                MessageBodyView(html: html, height: $bodyHeight)
                    .frame(height: bodyHeight)
            } else {
                Text(message.textBody ?? "")
                    .font(.callout)
                    .textSelection(.enabled)
            }
        }
        .padding(16)
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
