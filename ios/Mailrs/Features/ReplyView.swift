import SwiftUI

/// Answering or passing on a thread.
///
/// A sheet rather than a push: a reply is a thing you finish or abandon,
/// and the message you are answering should still be behind it. The
/// mode picker is the web's three (`reply-box.tsx`): Reply, Reply All,
/// Forward — same recipient rules, same wire shapes.
struct ReplyView: View {
    enum Mode: String, CaseIterable, Identifiable {
        case reply = "Reply"
        case replyAll = "Reply All"
        case forward = "Forward"
        var id: String { rawValue }
    }

    let thread: Wire.Conversation
    /// The message being answered — the last one in the thread.
    let replyingTo: Wire.Message?
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var mode: Mode = .reply
    @State private var body_ = ""
    /// Forward's To line, typed by hand — a forward has no one to
    /// derive it from.
    @State private var forwardTo = ""
    @State private var sending = false
    /// Send succeeded: the words left as mail, so the sheet's closing
    /// must not refile them as a draft. Without this, onDisappear's
    /// save ran after the send's delete and quietly resurrected it.
    @State private var didSend = false
    @State private var failure: String?
    @State private var suggestions: [String] = []
    @State private var suggestionTask: Task<Void, Never>?
    /// The same autosave contract as compose: one id per session,
    /// upserted — a half-written reply must survive the app dying.
    @State private var draftId: Int64?
    @State private var autosave: Task<Void, Never>?
    @State private var attachments: [MultipartForm.FilePart] = []
    @FocusState private var bodyFocused: Bool

    /// The addr-specs, not the display forms: the server takes bare
    /// addresses, and "Alice Smith <alice@…>" as a recipient entry is
    /// trusting every hop to re-parse what we already parsed.
    private var recipients: [String] {
        guard let replyingTo else { return [] }
        switch mode {
        case .reply:
            return ReplyRecipients.reply(toSender: replyingTo.sender)
        case .replyAll:
            return ReplyRecipients.replyAll(
                sender: replyingTo.sender,
                recipients: replyingTo.recipients,
                myAddress: session.myAddress
            )
        case .forward:
            return forwardTo
                .split(whereSeparator: { $0 == "," || $0 == ";" })
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty }
        }
    }

    private var subject: String {
        ReplyRecipients.subject(thread.subject, forwarding: mode == .forward)
    }

    private var sendDisabled: Bool {
        if sending { return true }
        if body_.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return true }
        return mode == .forward && recipients.isEmpty
    }

    var body: some View {
        NavigationStack {
            Form {
                Picker("Mode", selection: $mode) {
                    ForEach(Mode.allCases) { mode in
                        Text(mode.rawValue).tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                .listRowBackground(Color.clear)

                Section("To") {
                    if mode == .forward {
                        TextField("addresses, comma-separated", text: $forwardTo)
                            .textInputAutocapitalization(.never)
                            .keyboardType(.emailAddress)
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("forward-to")
                        ContactSuggestions(text: $forwardTo, suggestions: $suggestions)
                    } else {
                        // Shown as names, sent as addresses.
                        Text(displayedRecipients)
                            .foregroundStyle(.secondary)
                    }
                }
                Section("Subject") {
                    Text(subject).foregroundStyle(.secondary)
                }
                Section("Message") {
                    TextEditor(text: $body_)
                        .frame(minHeight: 180)
                        .focused($bodyFocused)
                }
                if mode == .forward, replyingTo != nil {
                    Section {
                        // The original does not appear here because it
                        // does not travel from here: the server appends
                        // body and attachments from the stored .eml.
                        Label("The original message and its attachments are included",
                              systemImage: "paperclip")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
                if !attachments.isEmpty {
                    Section {
                        AttachmentRows(attachments: $attachments)
                    }
                }
                if let failure {
                    Section {
                        Text(failure).foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle(mode.rawValue)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    AttachMenu(attachments: $attachments)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") { Task { await send() } }
                        .disabled(sendDisabled)
                }
            }
            .onAppear {
                // Fetch first: on a cold start nothing has loaded the
                // drafts list yet. The restore keeps its empty-body
                // guard, so typing during the fetch wins.
                Task {
                    await session.loadDrafts()
                    restoreDraft()
                }
                bodyFocused = true
            }
            .onChange(of: forwardTo) { _, text in
                suggestionTask = ContactSuggestions.schedule(
                    replacing: suggestionTask, for: text, in: session
                ) { suggestions = $0 }
            }
            .onChange(of: body_) { _, _ in scheduleAutosave() }
            .onDisappear {
                autosave?.cancel()
                // Cancel is not "discard", same as compose: closing the
                // sheet with something typed saves it.
                saveNow()
            }
        }
    }

    private var displayedRecipients: String {
        guard let replyingTo else { return "" }
        switch mode {
        case .reply:
            return SenderName.extractName(replyingTo.sender)
        case .replyAll, .forward:
            return recipients.joined(separator: ", ")
        }
    }

    /// The newest draft filed under this thread, put back where the
    /// typing stopped. Body only — the draft does not record which
    /// mode it was typed in, and reply is the mode you resume into.
    private func restoreDraft() {
        guard body_.isEmpty else { return }
        let mine = session.drafts
            .filter { $0.replyToThreadId == thread.threadId }
            .max { $0.updatedAt < $1.updatedAt }
        guard let mine else { return }
        body_ = mine.body
        draftId = mine.id
    }

    private func scheduleAutosave() {
        autosave?.cancel()
        autosave = Task {
            try? await Task.sleep(for: .seconds(2))
            guard !Task.isCancelled else { return }
            await save()
        }
    }

    private func save() async {
        // Body only: a reply's subject is always prefilled, so counting
        // it would file a draft for every sheet opened and closed.
        guard DraftRule.isWorthSaving(to: "", subject: "", body: body_) else { return }
        draftId = await session.saveDraft(
            id: draftId,
            to: recipients.joined(separator: ", "),
            subject: subject,
            body: body_,
            replyToThreadId: thread.threadId
        )
    }

    private func saveNow() {
        guard !didSend else { return }
        guard DraftRule.isWorthSaving(to: "", subject: "", body: body_) else { return }
        let (title, text, id, tid, addrs) =
            (subject, body_, draftId, thread.threadId, recipients.joined(separator: ", "))
        Task {
            _ = await session.saveDraft(
                id: id, to: addrs, subject: title, body: text, replyToThreadId: tid
            )
            await session.loadDrafts()
        }
    }

    private func send() async {
        sending = true
        failure = nil
        do {
            switch mode {
            case .reply, .replyAll:
                try await session.sendReply(
                    to: recipients,
                    subject: subject,
                    body: body_,
                    inReplyTo: replyingTo?.messageId,
                    threadId: thread.threadId,
                    attachments: attachments
                )
            case .forward:
                guard let replyingTo else { return }
                try await session.sendForward(
                    to: recipients,
                    subject: subject,
                    body: body_,
                    forwardMessageId: replyingTo.messageId,
                    forwardAttachmentsFrom: replyingTo.uid,
                    attachments: attachments
                )
            }
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
