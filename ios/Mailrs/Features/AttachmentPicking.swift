import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// The attachment UI both send sheets share — compose and reply must
/// not grow separate attach behaviours, any more than their To fields
/// grew separate autocompletes.

/// The removable file rows above the composer.
struct AttachmentRows: View {
    @Binding var attachments: [MultipartForm.FilePart]

    var body: some View {
        ForEach(Array(attachments.enumerated()), id: \.offset) { index, file in
            HStack {
                Image(systemName: "paperclip")
                    .foregroundStyle(.secondary)
                Text(file.filename)
                    .font(.subheadline)
                    .lineLimit(1)
                Spacer()
                Text(file.data.count.formatted(.byteCount(style: .file)))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button {
                    _ = withAnimation { attachments.remove(at: index) }
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Remove \(file.filename)")
            }
        }
    }
}

/// The paperclip menu for a toolbar — the bar, not the form, because
/// the keyboard covers the form mid-compose and a tap on a covered
/// menu opens nothing.
struct AttachMenu: View {
    @Binding var attachments: [MultipartForm.FilePart]
    @State private var pickedPhoto: PhotosPickerItem?
    @State private var importingFile = false

    var body: some View {
        Menu {
            PhotosPicker(selection: $pickedPhoto, matching: .images) {
                Label("Photo library", systemImage: "photo")
            }
            Button {
                importingFile = true
            } label: {
                Label("Choose a file", systemImage: "folder")
            }
            if ProcessInfo.processInfo.arguments.contains("-mailrsToken") {
                // Test-only: the system pickers are separate processes
                // XCUITest cannot reach, so the wire path gets its own
                // way in.
                Button("Attach sample file") {
                    attachments.append(.init(
                        name: "attachments", filename: "sample.txt",
                        contentType: "text/plain",
                        data: Data("sample attachment".utf8)
                    ))
                }
            }
        } label: {
            Label("Attach", systemImage: "paperclip")
        }
        .onChange(of: pickedPhoto) { _, item in
            guard let item else { return }
            Task {
                // Bytes plus a best-effort type; a photo that fails to
                // load attaches nothing rather than an empty file.
                guard let data = try? await item.loadTransferable(type: Data.self) else { return }
                let type = item.supportedContentTypes.first
                let ext = type?.preferredFilenameExtension ?? "jpg"
                withAnimation {
                    attachments.append(.init(
                        name: "attachments",
                        filename: "photo-\(attachments.count + 1).\(ext)",
                        contentType: type?.preferredMIMEType ?? "image/jpeg",
                        data: data
                    ))
                }
                pickedPhoto = nil
            }
        }
        .fileImporter(isPresented: $importingFile, allowedContentTypes: [.item]) { result in
            guard case let .success(url) = result else { return }
            // Security-scoped: without the access pair the read fails
            // on real devices and quietly works in the simulator,
            // which is the worst kind of passing.
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            guard let data = try? Data(contentsOf: url) else { return }
            let type = UTType(filenameExtension: url.pathExtension)
            withAnimation {
                attachments.append(.init(
                    name: "attachments",
                    filename: url.lastPathComponent,
                    contentType: type?.preferredMIMEType ?? "application/octet-stream",
                    data: data
                ))
            }
        }
    }
}
