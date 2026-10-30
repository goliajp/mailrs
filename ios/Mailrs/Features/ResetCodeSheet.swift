import SwiftUI

/// Setting a new password on an account whose password an administrator
/// cleared, with the code an issuer handed its owner in person. Nothing is
/// signed in: the person signs in with the new password afterwards.
struct ResetCodeSheet: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss
    let initialAddress: String
    /// Called with the address once the new password is set.
    let onDone: (String) -> Void

    @State private var address = ""
    @State private var code = ""
    @State private var password = ""
    /// The new password shown as typed, to check it before it is set.
    @State private var showsPassword = false
    @State private var failure: String?
    @State private var busy = false

    private var whom: String {
        session.resetContact?.text ?? String(localized: "an administrator")
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("Your password was reset by an administrator. Ask \(whom) for a reset code, then set a new password here.")
                        .font(.subheadline)
                        .accessibilityIdentifier("text.resetExplain")
                }
                Section {
                    TextField("Email", text: $address)
                        .textContentType(.username)
                        .neverCapitalised()
                        .mailKeyboard()
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("field.resetAddress")
                    TextField("Reset code", text: $code)
                        .textContentType(.oneTimeCode)
                        .neverCapitalised()
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("field.resetCode")
                    // no .newPassword content type: the strong-password
                    // offer it brings up took focus at every key, and the
                    // secure field emptied itself each time it got it back
                    if showsPassword {
                        TextField("New password", text: $password)
                            .neverCapitalised()
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("field.resetPassword")
                    } else {
                        SecureField("New password", text: $password)
                            .accessibilityIdentifier("field.resetPassword")
                    }
                    Toggle("Show password", isOn: $showsPassword)
                        .accessibilityIdentifier("toggle.resetShowPassword")
                } footer: {
                    Text("At least 12 characters. Leave out your name and the company name.")
                }
                if let failure {
                    Text(failure)
                        .foregroundStyle(.red)
                        .accessibilityIdentifier("text.resetFailure")
                }
            }
            .navigationTitle("Set a new password")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    // not disabled by what is typed: a secure field that
                    // loses focus to that redraw empties itself when it gets
                    // it back, so the first password kept one character
                    Button("Set") { Task { await submit() } }
                        .disabled(busy)
                        .accessibilityIdentifier("button.resetSubmit")
                }
            }
            .onAppear { address = initialAddress }
        }
    }

    private func submit() async {
        guard !address.isEmpty, !code.isEmpty, !password.isEmpty else {
            failure = String(localized: "Fill in the address, the code and the new password.")
            return
        }
        busy = true
        defer { busy = false }
        let trimmed = address.trimmingCharacters(in: .whitespaces)
        do {
            try await session.resetPassword(address: trimmed, code: code, newPassword: password)
            failure = nil
            onDone(trimmed)
            dismiss()
        } catch MailrsError.invalidResetCode {
            failure = String(localized: "That code does not work: it may be mistyped, used up, or replaced by a newer one. Ask \(whom) for a new code.")
        } catch {
            failure = error.localizedDescription
        }
    }
}
