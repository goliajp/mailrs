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

        /// The raw value is the wire-ish identity and the English key;
        /// this is what the segment shows, which has to go through a
        /// table to change language.
        var label: LocalizedStringKey { LocalizedStringKey(rawValue) }
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
    /// Copies the writer adds. A reply cannot derive them: the wire
    /// carries the original's To line and nothing else, so the Cc of the
    /// message being answered is not knowable here — inventing one from
    /// the To line would address people the sender had merely written to,
    /// not copied.
    @State private var cc = ""
    @State private var bcc = ""
    @State private var showsCopies = false
    @FocusState private var focus: ComposerField?

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

    /// Forward types its own recipients; the other two modes derive
    /// them, and a field would invite an edit that goes nowhere.
    private var toSlot: ComposerSlot {
        if mode == .forward { return .editable($forwardTo) }
        return .fixed(displayedRecipients)
    }

    private var sendDisabled: Bool {
        if sending { return true }
        if body_.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return true }
        return mode == .forward && recipients.isEmpty
    }

    var body: some View {
        NavigationStack {
            // Not a Form: a Form spends a section header, a card and
            // two paddings on each of To and Subject, and the editor
            // — the only thing anyone came here to touch — started
            // three hundred points down, below the fold once the
            // keyboard was up. These are one compact line each, and
            // the editor takes everything that is left.
            VStack(spacing: 0) {
                Picker("Mode", selection: $mode) {
                    ForEach(Mode.allCases) { mode in
                        Text(mode.label).tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                .padding(.horizontal, 12)
                .padding(.bottom, 8)

                ComposerHeader(
                    to: toSlot, cc: $cc, bcc: $bcc,
                    subject: .fixed(subject),
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
                if mode == .forward, replyingTo != nil {
                    Divider()
                    // The original does not appear here because it does
                    // not travel from here: the server appends body and
                    // attachments from the stored .eml.
                    Label("The original message and its attachments are included",
                          systemImage: "paperclip")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
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
                focus = .body
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
        cc = mine.cc
        bcc = mine.bcc
        // Copies that came back with the draft are shown, not hidden:
        // folded-away fields with addresses in them are people the
        // writer cannot see they are about to write to.
        showsCopies = !mine.cc.isEmpty || !mine.bcc.isEmpty
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
            cc: cc, bcc: bcc,
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
        let (copies, blind) = (cc, bcc)
        Task {
            _ = await session.saveDraft(
                id: id, to: addrs, cc: copies, bcc: blind,
                subject: title, body: text, replyToThreadId: tid
            )
            await session.loadDrafts()
        }
    }

    /// What was typed, with the message being answered beneath it.
    ///
    /// Forward carries the original from the server's stored .eml, so it
    /// is left alone; a reply had nothing at all.
    private var quotedBody: String {
        guard let replyingTo else { return body_ }
        return ReplyQuote.body(
            typed: body_,
            from: SenderName.extractName(replyingTo.sender),
            date: RowDate.stamp(epochSeconds: replyingTo.internalDate),
            // The plain half, never the HTML: a reply quoting a
            // newsletter's markup would carry its tables and its
            // colours back to the sender.
            original: replyingTo.textBody ?? ""
        )
    }

    private func send() async {
        sending = true
        failure = nil
        do {
            switch mode {
            case .reply, .replyAll:
                try await session.sendReply(
                    to: recipients,
                    cc: AddressList.parse(cc),
                    bcc: AddressList.parse(bcc),
                    subject: subject,
                    body: quotedBody,
                    inReplyTo: replyingTo?.messageId,
                    threadId: thread.threadId,
                    attachments: attachments
                )
            case .forward:
                guard let replyingTo else { return }
                try await session.sendForward(
                    to: recipients,
                    cc: AddressList.parse(cc),
                    bcc: AddressList.parse(bcc),
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

/// A compose header row: the field's name, then its value, on one line.
///
/// Apple Mail's shape. A `Form` section per field spends a header, a
/// card and two paddings to say the same thing, and every point it
/// spends pushes the editor further under the keyboard.
struct HeaderLine: View {
    let label: LocalizedStringKey
    let value: String

    var body: some View {
        HStack(spacing: 6) {
            Text(label)
                .foregroundStyle(.secondary)
            Text(verbatim: value)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .font(.subheadline)
        .padding(.horizontal, 12)
        .padding(.vertical, 7)
    }
}
