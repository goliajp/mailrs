import SwiftUI

/// Replying to a thread.
///
/// A sheet rather than a push: a reply is a thing you finish or abandon,
/// and the message you are answering should still be behind it.
struct ReplyView: View {
    let thread: Wire.Conversation
    /// The message being answered — the last one in the thread.
    let replyingTo: Wire.Message?
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var body_ = ""
    @State private var sending = false
    @State private var failure: String?
    @FocusState private var bodyFocused: Bool

    /// Everyone on the last message except yourself, so a reply does not
    /// arrive addressed back at the person sending it.
    private var recipients: [String] {
        guard let replyingTo else { return [] }
        // The addr-spec, not the display form: the server takes bare
        // addresses, and "Alice Smith <alice@…>" as a recipient entry
        // is trusting every hop to re-parse what we already parsed.
        return [SenderName.extractEmail(replyingTo.sender)]
    }

    private var subject: String {
        let existing = thread.subject
        if existing.lowercased().hasPrefix("re:") { return existing }
        return existing.isEmpty ? "Re:" : "Re: \(existing)"
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("To") {
                    // Shown as the name, sent as the address.
                    Text(replyingTo.map { SenderName.extractName($0.sender) } ?? "")
                        .foregroundStyle(.secondary)
                }
                Section("Subject") {
                    Text(subject).foregroundStyle(.secondary)
                }
                Section("Message") {
                    TextEditor(text: $body_)
                        .frame(minHeight: 180)
                        .focused($bodyFocused)
                }
                if let failure {
                    Section {
                        Text(failure).foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("Reply")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") { Task { await send() } }
                        .disabled(body_.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || sending)
                }
            }
            .onAppear { bodyFocused = true }
        }
    }

    private func send() async {
        sending = true
        failure = nil
        do {
            try await session.sendReply(
                to: recipients,
                subject: subject,
                body: body_,
                inReplyTo: replyingTo?.messageId,
                threadId: thread.threadId
            )
            dismiss()
        } catch {
            failure = error.localizedDescription
        }
        sending = false
    }
}
