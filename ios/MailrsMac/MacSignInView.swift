import SwiftUI

/// The front door, as a Mac window rather than a phone form.
///
/// What was here before was `SignInView`, the phone's screen, and it
/// arrived looking broken: on this platform a `TextField`'s title is
/// rendered as a **label beside the field**, so every placeholder —
/// `you@example.com`, `https://mail.example.com` — hung off the left
/// edge in blue, and the fields themselves stretched the full width of
/// a window that can be twelve hundred points across.
///
/// A sign-in on a Mac is a small centred panel: fixed width, labels
/// above their fields, the placeholder *inside* the field where it
/// belongs, and Return submitting.
struct MacSignInView: View {
    @Environment(Session.self) private var session
    @State private var address = ""
    @State private var password = ""
    @State private var totpCode = ""
    /// The password field appears unless Touch ID can answer for it.
    @State private var showsPassword = true

    /// Absent rather than empty: the server reads an empty code as a
    /// wrong one, and "I have no code" is a different claim.
    private var submittedTotp: String? {
        if totpCode.isEmpty { return nil }
        return totpCode
    }

    private var canSubmit: Bool {
        !address.isEmpty && !password.isEmpty && session.state != .signingIn
    }

    var body: some View {
        @Bindable var session = session
        VStack(spacing: 22) {
            VStack(spacing: 8) {
                BrandMark(size: 64)
                Text("Mailrs")
                    .font(.system(.title, design: .rounded, weight: .bold))
            }

            VStack(alignment: .leading, spacing: 14) {
                field("Account") {
                    // `prompt:`, and the label hidden: the title of a
                    // `TextField` becomes a leading label on this
                    // platform, which is how the placeholder ended up
                    // outside the field.
                    TextField("", text: $address,
                              prompt: Text(verbatim: "you@example.com"))
                        .textContentType(.username)
                        .accessibilityIdentifier("mac.signin.address")
                }
                if showsPassword {
                    field("Password") {
                        SecureField("", text: $password,
                                    prompt: Text("Password"))
                            .textContentType(.password)
                            .accessibilityIdentifier("mac.signin.password")
                    }
                }
                if session.needsTotp {
                    field("Six-digit code") {
                        TextField("", text: $totpCode,
                                  prompt: Text(verbatim: "000000"))
                            .accessibilityIdentifier("mac.signin.totp")
                    }
                }
                // Shown, not hidden in Preferences: the address decides
                // which mailbox this is, and getting it wrong is the
                // difference between "wrong password" and "wrong
                // server".
                field("Server") {
                    TextField("", text: Binding(
                        get: { session.baseURL.absoluteString },
                        set: { if let url = URL(string: $0) { session.baseURL = url } }),
                        prompt: Text(verbatim: "https://mail.example.com"))
                        .accessibilityIdentifier("mac.signin.server")
                }
            }

            if case let .failed(message) = session.state {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("mac.signin.error")
            }

            VStack(spacing: 10) {
                Button {
                    Task {
                        await session.signIn(address: address, password: password,
                                             totpCode: submittedTotp)
                    }
                } label: {
                    Group {
                        if session.state == .signingIn {
                            ProgressView().controlSize(.small)
                        } else {
                            Text("Sign in")
                        }
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                // Return submits, which is what every other panel on
                // this machine does.
                .keyboardShortcut(.defaultAction)
                .disabled(!canSubmit)
                .accessibilityIdentifier("mac.signin.submit")

                if !showsPassword {
                    Button {
                        Task { await signInWithBiometrics() }
                    } label: {
                        // "Unlock", not the settings screen's "Require
                        // Touch ID": this is the action, not the
                        // preference. And an SF Symbol, because
                        // `BiometricLock.kind().symbol` is Lucide path
                        // data for the phone's own row style.
                        Label("Unlock", systemImage: "touchid")
                    }
                    .accessibilityIdentifier("mac.signin.biometric")
                    Button("Use password instead") { showsPassword = true }
                        .buttonStyle(.link)
                }
            }
        }
        .padding(28)
        // A panel, not a page: the window is as wide as the desk allows
        // and this is a form with four short fields in it.
        .frame(width: 360)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task { await restoreLastAccount() }
    }

    @ViewBuilder
    private func field(_ label: LocalizedStringKey,
                       @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.callout)
                .foregroundStyle(.secondary)
            content()
                .textFieldStyle(.roundedBorder)
                .labelsHidden()
        }
    }

    /// Who it was last time, and whether this Mac can answer for them.
    private func restoreLastAccount() async {
        guard let last = CredentialStore.lastAddress else { return }
        address = last
        // A driven launch always types its password: the Touch ID
        // prompt is a system panel no test can answer, and a credential
        // left behind by an earlier run otherwise hides the field the
        // sign-in test types into.
        guard !ProcessInfo.processInfo.arguments.contains("-mailrsBaseURL") else { return }
        showsPassword = !(BiometricLock.isAvailable && CredentialStore.has(address: last))
    }

    /// Sign in with the stored credential.
    ///
    /// Three ways back to the password field, and all three are the
    /// same sentence: this cannot answer for you right now. Nothing
    /// stored, the look not passed, or the server refusing what was
    /// stored — the last one also forgets it, because a password that
    /// has changed will not start working again.
    private func signInWithBiometrics() async {
        let account = CredentialStore.lastAddress ?? address
        guard !account.isEmpty,
              let stored = CredentialStore.password(
                for: account, reason: String(localized: "Sign in to your mail"))
        else {
            showsPassword = true
            return
        }
        address = account
        await session.signIn(address: account, password: stored, totpCode: submittedTotp)
        if case .failed = session.state {
            CredentialStore.remove(address: account)
            showsPassword = true
        }
    }
}
