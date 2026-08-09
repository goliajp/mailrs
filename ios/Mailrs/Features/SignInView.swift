import SwiftUI

struct SignInView: View {
    @Environment(Session.self) private var session
    @State private var address = ""
    @State private var password = ""
    @State private var totpCode = ""
    /// The password field appears unless Face ID can answer for it.
    @State private var showsPassword = true

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

    /// Absent rather than empty: the server reads an empty code as a
    /// wrong one, and "I have no code" is a different claim.
    private var submittedTotp: String? {
        if totpCode.isEmpty { return nil }
        return totpCode
    }

    var body: some View {
        NavigationStack {
            Form {
                // The brand, before the form: the mark from the app
                // icon in accent, the name in rounded — a front door,
                // not a settings page.
                Section {
                    VStack(spacing: 10) {
                        // The icon's own artwork, not a tinted system
                        // glyph: the sign-in screen and the home screen
                        // are the first two things anyone sees, and
                        // they were different colours.
                        BrandMark(size: 76)
                        Text("Mailrs")
                            .font(.system(.largeTitle, design: .rounded, weight: .bold))
                        Text("GOLIA mail, in your pocket")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 12)
                    .listRowBackground(Color.clear)
                }
                .accessibilityElement(children: .combine)

                Section("Account") {
                    // `.username`, not `.emailAddress`, even though it is
                    // one: `.emailAddress` only picks the keyboard and
                    // offers addresses from Contacts. A field iOS will
                    // save a password *against* has to be the username
                    // half of a credential, and without it the offer to
                    // save one never appears.
                    TextField("you@example.com", text: $address)
                        .textContentType(.username)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    // Hidden while Face ID can stand in for it, and
                    // back the moment it cannot: no stored credential,
                    // one the server refused, or a look that did not
                    // pass.
                    if showsPassword {
                        SecureField("Password", text: $password)
                            .textContentType(.password)
                    }
                    if session.needsTotp {
                        TextField("Six-digit code", text: $totpCode)
                            .textContentType(.oneTimeCode)
                            .keyboardType(.numberPad)
                    }
                }

                if !showsPassword {
                    Section {
                        Button {
                            Task { await signInWithBiometrics() }
                        } label: {
                            LucideRow(title: BiometricLock.kind().label,
                                      icon: BiometricLock.kind().symbol)
                        }
                        .accessibilityIdentifier("sign-in-biometric")
                        Button("Use password instead") { showsPassword = true }
                            .foregroundStyle(.secondary)
                    }
                }

                Section("Server") {
                    // Shown, not hidden in a settings screen: the address
                    // decides which mailbox this is, and getting it wrong
                    // is the difference between "wrong password" and
                    // "wrong server".
                    TextField("https://mail.example.com", text: Binding(
                        get: { session.baseURL.absoluteString },
                        set: { if let url = URL(string: $0) { session.baseURL = url } }
                    ))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                }

                if case let .failed(message) = session.state {
                    Section {
                        Text(message).foregroundStyle(.red)
                    }
                }

                Section {
                    Button {
                        Task {
                            await session.signIn(
                                address: address,
                                password: password,
                                totpCode: submittedTotp
                            )
                        }
                    } label: {
                        // Full width and centered: the front door's one
                        // action should not read as another table row.
                        Group {
                            if session.state == .signingIn {
                                ProgressView().tint(.white)
                            } else {
                                Text("Sign in").fontWeight(.semibold)
                            }
                        }
                        .frame(maxWidth: .infinity)
                        .frame(height: 22)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .disabled(address.isEmpty || password.isEmpty || session.state == .signingIn)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
                }
            }
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .task {
                // Who it was last time, and whether the phone can answer
                // for them. A form that has forgotten the account on
                // every launch is the one thing a sign-in screen should
                // not do.
                guard let last = CredentialStore.lastAddress else { return }
                address = last
                // A driven launch always types its password. The Face ID
                // sheet is a system prompt no test can answer, and a
                // credential left on the simulator by an earlier run
                // otherwise hides the field the sign-in test types into.
                guard !ProcessInfo.processInfo.arguments.contains("-mailrsBaseURL") else {
                    return
                }
                showsPassword = !(BiometricLock.isAvailable
                    && CredentialStore.has(address: last))
            }
        }
    }
}
