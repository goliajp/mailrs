import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// Writing a new message.
///
/// Shares `Wire.SendRequest` with the reply path, with both threading
/// fields nil — a new message is not in a thread, and sending it with a
/// `reply_to_thread_id` would file it inside one.
struct ComposeView: View {
    /// A draft being resumed, if this is not a blank compose.
    var resuming: Wire.Draft?

    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var to = ""
    @State private var subject = ""
    @State private var body_ = ""
    @State private var sending = false
    /// Send succeeded: the words left as mail, so the sheet's closing
    /// must not refile them as a draft. Without this, onDisappear's
    /// save ran after the send's delete and quietly resurrected it.
    @State private var didSend = false
    @State private var failure: String?
    /// The id the server gave this session's draft. Held so every
    /// autosave upserts the same one instead of creating another.
    @State private var draftId: Int64?
    @State private var autosave: Task<Void, Never>?
    @State private var suggestions: [String] = []
    @State private var suggestionTask: Task<Void, Never>?
    @State private var attachments: [MultipartForm.FilePart] = []
    @State private var pickedPhoto: PhotosPickerItem?
    @State private var importingFile = false
    @FocusState private var focus: Field?

    private enum Field { case to, subject, body }

    var body: some View {
        NavigationStack {
            Form {
                Section("To") {
                    TextField("someone@example.com", text: $to)
                        .textContentType(.emailAddress)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .focused($focus, equals: .to)
                    ContactSuggestions(text: $to, suggestions: $suggestions)
                }
                Section("Subject") {
                    TextField("Subject", text: $subject)
                        .focused($focus, equals: .subject)
                }
                Section("Message") {
                    TextEditor(text: $body_)
                        .frame(minHeight: 200)
                        .focused($focus, equals: .body)
                }
                if !attachments.isEmpty {
                    Section {
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
                if let failure {
                    Section { Text(failure).foregroundStyle(.red) }
                }
            }
            .navigationTitle("New message")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    // Gmail's paperclip position: in the bar, where the
                    // keyboard cannot cover it mid-compose.
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
                            // Test-only: the system pickers are separate
                            // processes XCUITest cannot reach, so the
                            // wire path gets its own way in.
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
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") { Task { await send() } }
                        // A subject or a body may be empty — plenty of
                        // real mail is one line with no subject — but a
                        // message with nowhere to go is not a message.
                        .disabled(!AddressList.isSendable(to) || sending)
                }
            }
            .onAppear {
                if let resuming {
                    to = resuming.to
                    subject = resuming.subject
                    body_ = resuming.body
                    draftId = resuming.id
                }
                focus = .to
            }
            .onChange(of: [to, subject, body_]) { _, _ in scheduleAutosave() }
            .onChange(of: to) { _, text in
                suggestionTask = ContactSuggestions.schedule(
                    replacing: suggestionTask, for: text, in: session
                ) { suggestions = $0 }
            }
            .onChange(of: pickedPhoto) { _, item in
                guard let item else { return }
                Task {
                    // The transferable is bytes plus a best-effort type;
                    // a photo that fails to load attaches nothing rather
                    // than an empty file.
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
                // Security-scoped: without the access pair the read
                // fails on real devices and quietly works in the
                // simulator, which is the worst kind of passing.
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
            .onDisappear {
                autosave?.cancel()
                // Cancel is not "discard": closing the composer with
                // something in it saves rather than throwing it away, and
                // the draft is what makes that safe.
                saveNow()
            }
        }
    }

    /// Debounced, because every keystroke changes the fields and a save
    /// per character is a POST per character.
    private func scheduleAutosave() {
        autosave?.cancel()
        autosave = Task {
            try? await Task.sleep(for: .seconds(2))
            guard !Task.isCancelled else { return }
            await save()
        }
    }

    private func saveNow() {
        guard !didSend else { return }
        guard DraftRule.isWorthSaving(to: to, subject: subject, body: body_) else { return }
        let (recipients, title, text, id) = (to, subject, body_, draftId)
        Task {
            _ = await session.saveDraft(
                id: id, to: recipients, subject: title, body: text, replyToThreadId: nil
            )
            await session.loadDrafts()
        }
    }

    private func save() async {
        guard DraftRule.isWorthSaving(to: to, subject: subject, body: body_) else { return }
        draftId = await session.saveDraft(
            id: draftId, to: to, subject: subject, body: body_, replyToThreadId: nil
        )
    }

    private func send() async {
        sending = true
        failure = nil
        do {
            try await session.sendNew(
                to: AddressList.parse(to), subject: subject, body: body_,
                attachments: attachments
            )
            // Sent, so it is no longer a draft. Cancel the pending
            // autosave first or it recreates the one just deleted.
            didSend = true
            autosave?.cancel()
            if let draftId {
                await session.deleteDraft(id: draftId)
                self.draftId = nil
            }
            dismiss()
        } catch {
            failure = error.localizedDescription
        }
        sending = false
    }
}
