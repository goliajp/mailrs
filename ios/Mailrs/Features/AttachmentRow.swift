import SwiftUI

/// A downloaded attachment on disk, identified for `.sheet(item:)`.
private struct PreviewFile: Identifiable {
    let url: URL
    var id: String { url.path }
}

/// One attachment, and the tap that opens it.
///
/// Downloading to a temp file and handing that to Quick Look, rather
/// than rendering the bytes: Quick Look already knows every format a
/// phone can show, and an attachment this app cannot preview is still
/// worth being able to share out of it.
struct AttachmentRow: View {
    let uid: UInt32
    /// Position in the message's attachment array — the only index the
    /// server accepts, since the wire carries none.
    let index: Int
    let attachment: Wire.Attachment

    @Environment(Session.self) private var session
    @State private var localURL: URL?
    @State private var loading = false
    @State private var failure: String?

    var body: some View {
        Button {
            Task { await open() }
        } label: {
            HStack(spacing: 10) {
                Image(systemName: icon)
                    .foregroundStyle(.secondary)
                    .frame(width: 20)
                VStack(alignment: .leading, spacing: 1) {
                    Text(attachment.filename)
                    .lineLimit(1)
                    .truncationMode(.middle)
                        // Two lines, and truncating in the middle: the
                        // part of a filename that distinguishes it from
                        // its neighbours is usually the end, and
                        // "Invoice_2026_08_…" tells you nothing.
                        .font(.footnote)
                        .lineLimit(2)
                        .truncationMode(.middle)
                        .multilineTextAlignment(.leading)
                    Text(sizeText)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if loading { ProgressView() }
            }
        }
        .buttonStyle(.plain)
        .sheet(item: Binding(
            get: { localURL.map(PreviewFile.init) },
            set: { if $0 == nil { localURL = nil } }
        )) { file in
            QuickLookSheet(url: file.url) { localURL = nil }
        }
        .alert("Could not open", isPresented: Binding(
            get: { failure != nil }, set: { if !$0 { failure = nil } }
        )) {
            Button("OK", role: .cancel) { failure = nil }
        } message: {
            Text(failure ?? "")
        }
    }

    private var sizeText: String {
        ByteCountFormatter.string(fromByteCount: Int64(attachment.size), countStyle: .file)
    }

    private var icon: String {
        if attachment.contentType.hasPrefix("image/") { return "photo" }
        if attachment.contentType.hasPrefix("video/") { return "film" }
        if attachment.contentType.hasPrefix("audio/") { return "waveform" }
        if attachment.contentType.contains("pdf") { return "doc.richtext" }
        if attachment.contentType.contains("zip") { return "doc.zipper" }
        return "doc"
    }

    private func open() async {
        guard !loading else { return }
        loading = true
        defer { loading = false }
        do {
            let data = try await session.attachment(uid: uid, index: index)
            // Named from the wire's filename rather than parsed out of
            // `Content-Disposition`: it is the same decoded string the
            // server puts in that header, and Quick Look picks its
            // renderer off the extension.
            let url = FileManager.default.temporaryDirectory
                .appendingPathComponent(AttachmentFile.safeName(for: attachment.filename))
            try data.write(to: url, options: .atomic)
            localURL = url
        } catch {
            failure = error.localizedDescription
        }
    }

}
