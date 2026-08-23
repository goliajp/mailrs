import SwiftUI

/// One message from a connected mailbox, open.
struct MessageView: View {
    let row: MailboxRow
    let account: MailAccount?
    /// Handed the loaded message, because the recipient, the subject
    /// and the threading all come out of headers the list never had.
    var onReply: ((MessageReader.Loaded, Bool) -> Void)?
    @Environment(\.dismiss) private var dismiss
    @Environment(\.theme) private var theme
    @State private var outcome: MessageReader.Outcome?
    /// The attachment being previewed, if any.
    @State private var previewing: PreviewFile?
    @State private var openFailure = ""

    /// A file on disk, identified for `.sheet(item:)`.
    private struct PreviewFile: Identifiable {
        let url: URL
        var id: String { url.path }
    }

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
                        Button("Reply") { onReply(loaded, false) }
                            .accessibilityIdentifier("message.reply")
                    }
                }
                ToolbarItem(placement: .secondaryAction) {
                    // Offered only when there is somebody else on it.
                    // "Reply all" over a message with one recipient
                    // does the same thing as Reply and invites the
                    // mistake it is named for.
                    if case let .loaded(loaded) = outcome, let onReply, hasOthers(loaded) {
                        Button("Reply all") { onReply(loaded, true) }
                            .accessibilityIdentifier("message.replyAll")
                    }
                }
            }
        }
        .sheet(item: $previewing) { file in
            QuickLookSheet(url: file.url) { previewing = nil }
        }
        .task {
            guard let account else {
                outcome = .failed("This mailbox is no longer connected")
                return
            }
            outcome = await MessageReader.load(account: account, row: row)
        }
    }

    /// Write the bytes out and preview them.
    ///
    /// A per-message directory, because **the filename is the
    /// sender's and is not unique**: two messages carrying
    /// `invoice.pdf` would otherwise overwrite each other's.
    private func open(_ attachment: MessageAttachments.Attachment) {
        openFailure = ""
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(AttachmentFile.safeName(for: row.id))
        do {
            try FileManager.default.createDirectory(
                at: directory, withIntermediateDirectories: true)
            let url = directory.appendingPathComponent(
                AttachmentFile.safeName(for: attachment.filename))
            try attachment.bytes.write(to: url, options: .atomic)
            previewing = PreviewFile(url: url)
        } catch {
            // Said rather than swallowed: a tap that does nothing at
            // all reads as a broken button.
            openFailure = "This attachment could not be opened."
        }
    }

    /// Whether anybody besides the sender and this account was on it.
    private func hasOthers(_ loaded: MessageReader.Loaded) -> Bool {
        guard let account else { return false }
        return !MailAddresses.replyAll(
            to: loaded.headers.to, cc: loaded.headers.cc,
            primary: ReplyDraft.recipient(loaded.headers), mine: account.address
        ).isEmpty
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
                if !loaded.attachments.isEmpty {
                    Divider()
                    ForEach(loaded.attachments) { attachment in
                        // A tap writes it out and hands it to Quick
                        // Look, which already knows every format a
                        // phone can show — and an attachment this app
                        // cannot preview is still worth being able to
                        // share out of it.
                        Button {
                            open(attachment)
                        } label: {
                            // Named, sized and typed — the three
                            // things somebody needs to decide whether
                            // to open it, and the size especially: a
                            // 40 MB file on a phone on mobile data is
                            // a decision, not a tap. `.byteCount` is
                            // what the rest of this app and the
                            // phone's own file manager use; a second
                            // formatter would be a second answer to
                            // one question.
                            VStack(alignment: .leading, spacing: 2) {
                                Text(attachment.filename).font(.subheadline)
                                Text(
                                    "\(attachment.mimeType) · "
                                        + attachment.size.formatted(.byteCount(style: .file))
                                )
                                .font(.caption2)
                                .foregroundStyle(theme.fgMuted)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .buttonStyle(.plain)
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("message.attachment")
                    }
                    if !openFailure.isEmpty {
                        Text(openFailure)
                            .font(.caption2)
                            .foregroundStyle(theme.fgMuted)
                    }
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
