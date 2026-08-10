import SwiftUI

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
    @State private var cc = ""
    @State private var bcc = ""
    /// Cc and Bcc are folded away until a message needs them, and stay
    /// open once a resumed draft turns out to have them.
    @State private var showsCopies = false
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
    @FocusState private var focus: ComposerField?

    var body: some View {
        NavigationStack {
            // Same shape as the reply sheet, for the same reason: a
            // Form's section per field pushed the body below the fold
            // once the keyboard was up, and the body is the only thing
            // anyone opened this to write.
            VStack(spacing: 0) {
                ComposerHeader(
                    to: .editable($to), cc: $cc, bcc: $bcc,
                    subject: .editable($subject),
                    showsCopies: $showsCopies, suggestions: $suggestions,
                    focus: $focus
                ) { text in
                    suggestionTask = ContactSuggestions.schedule(
                        replacing: suggestionTask, for: text, in: session
                    ) { suggestions = $0 }
                }

                ComposerEditor(text: $body_, placeholder: "Message")
                    .focused($focus, equals: .body)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)

                if !attachments.isEmpty {
                    Divider()
                    VStack(spacing: 4) {
                        AttachmentRows(attachments: $attachments)
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                }
                if let failure {
                    Divider()
                    Text(failure)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 6)
                }
            }
            .padding(.top, 8)
            .navigationTitle("New message")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("composer-cancel")
                }
                ToolbarItem(placement: .topBarTrailing) {
                    AttachMenu(attachments: $attachments)
                }
                // Its own `ToolbarItem`, not sharing one with Send.
                // Two controls inside a single item are composed into
                // one element, and `buttons["Send"]` — which ten tests
                // use — then matches nothing at all.
                ToolbarItem(placement: .topBarTrailing) {
                    // Two controls, not one menu with a primary
                    // action: a `Menu(primaryAction:)` labelled "Send"
                    // is a button to a person and a menu to anything
                    // driving the app, and every test that taps Send
                    // sat waiting for a message the menu had swallowed.
                    Menu {
                        ForEach(SendSchedule.allCases.filter { $0 != .now }) { option in
                            Button(option.label) {
                                Task { await send(schedule: option) }
                            }
                        }
                    } label: {
                        Label("Send later", systemImage: "clock")
                    }
                    .disabled(!AddressList.isSendable(to) || sending)
                }
                ToolbarItem(placement: .confirmationAction) {
                    // No `accessibilityIdentifier`: it *replaces* the
                    // label as the element's identity, and adding one
                    // here made `buttons["Send"]` match nothing.
                    Button("Send") { Task { await send(schedule: .now) } }
                        // A subject or a body may be empty — plenty of
                        // real mail is one line with no subject — but a
                        // message with nowhere to go is not a message.
                        .disabled(!AddressList.isSendable(to) || sending)
                }
            }
            .onAppear {
                if let resuming {
                    to = resuming.to
                    cc = resuming.cc
                    bcc = resuming.bcc
                    // A draft that carried copies opens showing them —
                    // folded-away fields with something in them are
                    // words the writer cannot see they are about to send.
                    showsCopies = !resuming.cc.isEmpty || !resuming.bcc.isEmpty
                    subject = resuming.subject
                    body_ = resuming.body
                    draftId = resuming.id
                }
                focus = .to
            }
            .onChange(of: [to, cc, bcc, subject, body_]) { _, _ in scheduleAutosave() }
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
        let (recipients, copies, blind) = (to, cc, bcc)
        let (title, text, id) = (subject, body_, draftId)
        Task {
            _ = await session.saveDraft(
                id: id, to: recipients, cc: copies, bcc: blind,
                subject: title, body: text, replyToThreadId: nil
            )
            await session.loadDrafts()
        }
    }

    private func save() async {
        guard DraftRule.isWorthSaving(to: to, subject: subject, body: body_) else { return }
        draftId = await session.saveDraft(
            id: draftId, to: to, cc: cc, bcc: bcc,
            subject: subject, body: body_, replyToThreadId: nil
        )
    }

    private func send(schedule: SendSchedule) async {
        sending = true
        failure = nil
        do {
            try await session.sendNew(
                to: AddressList.parse(to), cc: AddressList.parse(cc),
                bcc: AddressList.parse(bcc), subject: subject,
                body: MailSignature.append(body: body_, signature: session.signature),
                attachments: attachments,
                scheduledAt: schedule.fireDate(after: Date(), calendar: .current)
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
