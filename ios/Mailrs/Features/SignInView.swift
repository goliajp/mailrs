import SwiftUI

struct SignInView: View {
    @Environment(Session.self) private var session
    @State private var address = ""
    @State private var password = ""
    @State private var totpCode = ""

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
                    TextField("you@example.com", text: $address)
                        .textContentType(.emailAddress)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    SecureField("Password", text: $password)
                        .textContentType(.password)
                    if session.needsTotp {
                        TextField("Six-digit code", text: $totpCode)
                            .textContentType(.oneTimeCode)
                            .keyboardType(.numberPad)
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
        }
    }
}
