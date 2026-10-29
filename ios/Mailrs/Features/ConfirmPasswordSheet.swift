import SwiftUI

/// The password prompt before a change the server asked to confirm.
///
/// Attached with `.confirmPrompt()` to each screen that makes such a
/// change: those screens are themselves sheets, and a sheet can only be
/// presented from the one on top.
struct ConfirmPasswordSheet: View {
    @Environment(Session.self) private var session
    @State private var password = ""
    @State private var code = ""

    var body: some View {
        let prompt = session.confirmGate.prompt ?? ConfirmGate.Prompt()
        NavigationStack {
            Form {
                Section {
                    SecureField("Password", text: $password)
                        .textContentType(.password)
                        .accessibilityIdentifier("field.confirmPassword")
                    if prompt.needsCode {
                        TextField("Authenticator code", text: $code)
                            .keyboardType(.numberPad)
                            .textContentType(.oneTimeCode)
                            .accessibilityIdentifier("field.confirmCode")
                    }
                } footer: {
                    Text("Enter your password to continue. You won’t be asked again for 10 minutes.")
                }
                if let message = prompt.message {
                    Text(message)
                        .foregroundStyle(.red)
                        .accessibilityIdentifier("text.confirmMessage")
                }
            }
            .navigationTitle("Confirm it’s you")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { session.cancelConfirmation() }
                        .accessibilityIdentifier("button.cancelConfirm")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Continue") {
                        let (p, c) = (password, code)
                        password = ""
                        Task { await session.submitConfirmation(password: p, code: c) }
                    }
                    .disabled(password.isEmpty || prompt.busy)
                    .accessibilityIdentifier("button.confirmPassword")
                }
            }
        }
        .presentationDetents([.medium])
    }
}

extension View {
    /// Show the password prompt over this screen when a change made from
    /// it is refused for want of a recent confirmation.
    func confirmPrompt() -> some View {
        modifier(ConfirmPrompting())
    }
}

private struct ConfirmPrompting: ViewModifier {
    @Environment(Session.self) private var session

    func body(content: Content) -> some View {
        content.sheet(
            isPresented: Binding(
                get: { session.confirmGate.prompt != nil },
                set: { shown in if !shown { session.cancelConfirmation() } }
            )
        ) {
            ConfirmPasswordSheet()
        }
    }
}
