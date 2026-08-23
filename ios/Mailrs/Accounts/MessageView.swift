import SwiftUI

/// One message from a connected mailbox, open.
struct MessageView: View {
    let row: MailboxRow
    let account: MailAccount?
    /// Handed the loaded message, because the recipient, the subject
    /// and the threading all come out of headers the list never had.
    var onReply: ((MessageReader.Loaded) -> Void)?
    @Environment(\.dismiss) private var dismiss
    @Environment(\.theme) private var theme
    @State private var outcome: MessageReader.Outcome?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    header
                    Divider()
                    content
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(16)
            }
            .navigationTitle("Message")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("message.done")
                }
                ToolbarItem(placement: .primaryAction) {
                    // Offered only once there is a message to reply
                    // to: everything a reply is made of arrives with
                    // the body.
                    if case let .loaded(loaded) = outcome, let onReply {
                        Button("Reply") { onReply(loaded) }
                            .accessibilityIdentifier("message.reply")
                    }
                }
            }
        }
        .task {
            guard let account else {
                outcome = .failed("This mailbox is no longer connected")
                return
            }
            outcome = await MessageReader.load(account: account, row: row)
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(row.displaySubject)
                .font(.headline)
                .accessibilityIdentifier("message.subject")
            Text(row.displaySender)
                .font(.subheadline)
                .foregroundStyle(theme.fgSecondary)
            Text(whenAndWhere)
                .font(.caption)
                .foregroundStyle(theme.fgMuted)
        }
    }

    private var whenAndWhere: String {
        var parts: [String] = []
        if let when = row.date {
            parts.append(
                Date(timeIntervalSince1970: TimeInterval(when))
                    .formatted(date: .abbreviated, time: .shortened))
        }
        var name = "Unknown"
        if let account { name = account.title }
        parts.append("\(name) · \(row.folder)")
        return parts.joined(separator: " · ")
    }

    @ViewBuilder private var content: some View {
        switch outcome {
        case nil:
            HStack(spacing: 8) {
                ProgressView()
                Text("Fetching…").font(.footnote).foregroundStyle(theme.fgMuted)
            }
        case let .failed(why):
            Text(why)
                .font(.footnote)
                .foregroundStyle(theme.fgMuted)
                .accessibilityIdentifier("message.failed")
        case let .loaded(loaded):
            VStack(alignment: .leading, spacing: 8) {
                if loaded.text.isEmpty {
                    Text("This message has no text to show.")
                        .font(.footnote)
                        .foregroundStyle(theme.fgMuted)
                } else {
                    // Selectable, because half of what people do with a
                    // message is copy a code or an address out of it.
                    Text(loaded.text)
                        .font(.body)
                        .textSelection(.enabled)
                        .accessibilityIdentifier("message.body")
                }
                if loaded.fromHTML {
                    // Said plainly rather than hidden: a formatted
                    // message shown as text reads as broken unless
                    // something says why, and the why is that no
                    // remote image gets to report that this was read.
                    Text("Shown as text. Images and formatting are not loaded.")
                        .font(.caption2)
                        .foregroundStyle(theme.fgMuted)
                }
            }
        }
    }
}
