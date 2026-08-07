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
    @State private var failure: String?
    @State private var suggestions: [String] = []
    @State private var suggestionTask: Task<Void, Never>?
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
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") { Task { await send() } }
                        .disabled(sendDisabled)
                }
            }
            .onAppear { bodyFocused = true }
            .onChange(of: forwardTo) { _, text in
                suggestionTask = ContactSuggestions.schedule(
                    replacing: suggestionTask, for: text, in: session
                ) { suggestions = $0 }
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
                    threadId: thread.threadId
                )
            case .forward:
                guard let replyingTo else { return }
                try await session.sendForward(
                    to: recipients,
                    subject: subject,
                    body: body_,
                    forwardMessageId: replyingTo.messageId,
                    forwardAttachmentsFrom: replyingTo.uid
                )
            }
            dismiss()
        } catch {
            failure = error.localizedDescription
        }
        sending = false
    }
}
