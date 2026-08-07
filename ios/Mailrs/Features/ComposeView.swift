import SwiftUI

/// Writing a new message.
///
/// Shares `Wire.SendRequest` with the reply path, with both threading
/// fields nil — a new message is not in a thread, and sending it with a
/// `reply_to_thread_id` would file it inside one.
struct ComposeView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var to = ""
    @State private var subject = ""
    @State private var body_ = ""
    @State private var sending = false
    @State private var failure: String?
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
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") { Task { await send() } }
                        // A subject or a body may be empty — plenty of
                        // real mail is one line with no subject — but a
                        // message with nowhere to go is not a message.
                        .disabled(!AddressList.isSendable(to) || sending)
                }
            }
            .onAppear { focus = .to }
        }
    }

    private func send() async {
        sending = true
        failure = nil
        do {
            try await session.sendNew(
                to: AddressList.parse(to), subject: subject, body: body_
            )
            dismiss()
        } catch {
            failure = error.localizedDescription
        }
        sending = false
    }
}
