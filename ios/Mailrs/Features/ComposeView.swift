import SwiftUI

/// Writing a new message.
///
/// Shares `Wire.SendRequest` with the reply path, with both threading
/// fields nil — a new message is not in a thread, and sending it with a
/// `reply_to_thread_id` would file it inside one.
struct ComposeView: View {
    /// A draft being resumed, if this is not a blank compose.
    var resuming: Wire.Draft?
    /// A sent message being edited before it goes again.
    ///
    /// Its attachments are carried rather than downloaded — the server
    /// holds the bytes and the send names which to keep by index — so
    /// they are described here and never become `FilePart`s.
    var redrafting: Wire.Redraft?

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
    /// Which address it leaves by, and everything it could be.
    @State private var from = ""
    @State private var fromAddresses: [FromAddress] = []
    /// The id the server gave this session's draft. Held so every
    /// autosave upserts the same one instead of creating another.
    @State private var draftId: Int64?
    @State private var autosave: Task<Void, Never>?
    @State private var suggestions: [String] = []
    @State private var suggestionTask: Task<Void, Never>?
    @State private var attachments: [MultipartForm.FilePart] = []
    /// Which carried files the reader has taken off this re-edit.
    @State private var droppedCarried: Set<Int> = []
    @FocusState private var focus: ComposerField?

    var body: some View {
        NavigationStack {
            // Same shape as the reply sheet, for the same reason: a
            // Form's section per field pushed the body below the fold
            // once the keyboard was up, and the body is the only thing
            // anyone opened this to write.
            VStack(spacing: 0) {
                FromPicker(addresses: fromAddresses, selection: $from)
                    .padding(.horizontal)
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

                // Files the server is holding for this re-edit. Listed
                // like any other attachment because that is what they
                // are to the reader, and removable — a re-edit that
                // could not drop a file would make "edit and send
                // again" mean "send the same thing with different
                // words".
                if let carried = redrafting?.attachments.filter({ !droppedCarried.contains($0.index) }),
                   !carried.isEmpty {
                    Divider()
                    VStack(spacing: 4) {
                        ForEach(carried) { file in
                            HStack(spacing: 8) {
                                Image(systemName: "paperclip")
                                    .foregroundStyle(.secondary)
                                Text(file.filename)
                                    .font(.footnote)
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                                Text(file.size.formatted(.byteCount(style: .file)))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                Button {
                                    droppedCarried.insert(file.index)
                                } label: {
                                    Image(systemName: "xmark.circle.fill")
                                        .foregroundStyle(.secondary)
                                }
                                .accessibilityLabel("Remove \(file.filename)")
                            }
                        }
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                }
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
                if let redrafting {
                    to = redrafting.to.joined(separator: ", ")
                    cc = redrafting.cc.joined(separator: ", ")
                    bcc = redrafting.bcc.joined(separator: ", ")
                    showsCopies = !redrafting.cc.isEmpty || !redrafting.bcc.isEmpty
                    subject = redrafting.subject
                    body_ = redrafting.body
                }
                focus = .to
            }
            .task {
                fromAddresses = await loadFromAddresses(
                    session: session, own: session.myAddress)
                if from.isEmpty { from = fromAddresses.first?.address ?? "" }
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

    /// Which carried files survive the edit, or `nil` when nothing was
    /// carried.
    ///
    /// `nil` and `[]` are not the same on the wire: absent keeps every
    /// carried attachment and an empty list keeps none. A compose that
    /// never carried anything must send absent, or the server reads
    /// "keep none" as an instruction about files it is not holding.
    private var keptCarried: [Int]? {
        guard let redrafting, !redrafting.attachments.isEmpty else { return nil }
        return redrafting.attachments.map(\.index).filter { !droppedCarried.contains($0) }
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
                scheduledAt: schedule.fireDate(after: Date(), calendar: .current),
                redraftOf: redrafting?.redraftOf,
                redraftKeep: keptCarried,
                from: from
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
