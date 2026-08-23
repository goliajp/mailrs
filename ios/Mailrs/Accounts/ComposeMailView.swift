import SwiftUI

/// Writing a message from a connected mailbox.
///
/// The From row is a picker even with one account, because which
/// address a message leaves by is the thing people get wrong and the
/// thing they cannot see afterwards. It is at the top, where every mail
/// client puts it.
struct ComposeMailView: View {
    let accounts: [MailAccount]
    let initial: OutgoingMessage.Draft
    let initialAccountId: String

    @Environment(\.dismiss) private var dismiss
    @Environment(\.theme) private var theme
    @State private var from: MailAccount?
    @State private var to = ""
    @State private var subject = ""
    @State private var message = ""
    @State private var sending = false
    @State private var failure = ""

    var body: some View {
        NavigationStack {
            Form {
                Section("From") {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(accounts) { account in
                                chip(account)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                }
                Section {
                    TextField("To", text: $to)
                        .textContentType(.emailAddress)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("compose.to")
                    TextField("Subject", text: $subject)
                        .accessibilityIdentifier("compose.subject")
                }
                Section {
                    TextEditor(text: $message)
                        .frame(minHeight: 200)
                        .accessibilityIdentifier("compose.body")
                }
                if !failure.isEmpty {
                    // In the form, not an alert that has to be
                    // dismissed before the message can be fixed — what
                    // went wrong and what to change are one screen.
                    Section {
                        Text(failure)
                            .font(.footnote)
                            .foregroundStyle(theme.fgMuted)
                            .accessibilityIdentifier("compose.failure")
                    }
                }
            }
            .navigationTitle("New message")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("compose.cancel")
                }
                ToolbarItem(placement: .primaryAction) {
                    if sending {
                        ProgressView()
                    } else {
                        Button("Send") { Task { await send() } }
                            .accessibilityIdentifier("compose.send")
                    }
                }
            }
        }
        .task {
            from = accounts.first { $0.id == initialAccountId } ?? accounts.first
            to = initial.to.joined(separator: ", ")
            subject = initial.subject
            message = initial.body
        }
    }

    private func chip(_ account: MailAccount) -> some View {
        let on = account.id == from?.id
        return Button {
            from = account
        } label: {
            HStack(spacing: 6) {
                Circle()
                    .fill(Color(hex: AccountColour.forId(account.id)))
                    .frame(width: 8, height: 8)
                Text(account.address).font(.footnote)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(chipBackground(on: on))
            .clipShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("compose.from.\(account.address)")
        .accessibilityAddTraits(chipTraits(on: on))
    }

    private func chipBackground(on: Bool) -> Color {
        if on { return theme.accent.opacity(0.18) }
        return theme.bgSecondary
    }

    private func chipTraits(on: Bool) -> AccessibilityTraits {
        if on { return .isSelected }
        return []
    }

    private func send() async {
        guard let account = from, !sending else { return }
        sending = true
        failure = ""
        var draft = initial
        draft.from = account.address
        draft.fromName = account.displayName
        draft.to = to.split(separator: ",").map {
            $0.trimmingCharacters(in: .whitespaces)
        }.filter { !$0.isEmpty }
        draft.subject = subject
        draft.body = message
        switch await AccountSender.send(draft, from: account) {
        case .sent: dismiss()
        case let .failed(why):
            failure = why
            sending = false
        }
    }
}
