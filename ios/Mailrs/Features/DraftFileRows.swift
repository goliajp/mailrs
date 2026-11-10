import SwiftUI

/// A downloaded draft file on disk, identified for `.sheet(item:)`.
private struct DraftPreview: Identifiable {
    let url: URL
    var id: String { url.path }
}

/// The files in a composer that keeps them with its draft. One the server
/// keeps opens through the draft file route; one still uploading, or
/// refused, is only in this window, and a refused one says why in the
/// server's words.
struct DraftFileRows: View {
    let files: DraftFiles

    @Environment(Session.self) private var session
    @State private var preview: DraftPreview?
    @State private var failure: String?

    var body: some View {
        if !files.isEmpty {
            Divider()
            VStack(alignment: .leading, spacing: 4) {
                ForEach(files.entries) { entry in
                    row(entry)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .sheet(item: $preview) { file in
                QuickLookSheet(url: file.url) { preview = nil }
            }
            .alert("Could not open", isPresented: Binding(
                get: { failure != nil }, set: { if !$0 { failure = nil } }
            )) {
                Button("OK", role: .cancel) { failure = nil }
            } message: {
                Text(failure ?? "")
            }
        }
    }

    private func row(_ entry: DraftFiles.Entry) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 8) {
                Image(systemName: "paperclip")
                    .foregroundStyle(.secondary)
                details(entry)
                Button {
                    withAnimation { files.remove(entry.id) }
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Remove \(entry.name)")
            }
            if let refusal = entry.refusal {
                Text(verbatim: refusal)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
    }

    /// Name and size; tapping them opens a file the server keeps.
    @ViewBuilder
    private func details(_ entry: DraftFiles.Entry) -> some View {
        if case let .stored(file) = entry.state {
            Button {
                Task { await open(file) }
            } label: {
                label(entry)
            }
            .buttonStyle(.plain)
        } else {
            label(entry)
        }
    }

    private func label(_ entry: DraftFiles.Entry) -> some View {
        HStack(spacing: 8) {
            Text(verbatim: entry.name)
                .font(.subheadline)
                .lineLimit(1)
                .truncationMode(.middle)
            if entry.isUploading {
                Text("Uploading…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            Text(entry.size.formatted(.byteCount(style: .file)))
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .contentShape(Rectangle())
    }

    private func open(_ file: Wire.DraftAttachment) async {
        do {
            let data = try await session.draftAttachment(file)
            let url = FileManager.default.temporaryDirectory
                .appendingPathComponent(AttachmentFile.safeName(for: file.name))
            try data.write(to: url, options: .atomic)
            preview = DraftPreview(url: url)
        } catch {
            failure = error.localizedDescription
        }
    }
}

enum DraftFileText {
    /// The save right before a send did not take the files shown, so the
    /// send would have gone with other files than the ones on screen.
    static var notSaved: String {
        AppText.string("The draft’s attachments could not be saved, so the message was not sent. Try again in a moment")
    }
}
