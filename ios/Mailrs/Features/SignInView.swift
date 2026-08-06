import SwiftUI

struct SignInView: View {
    @Environment(Session.self) private var session
    @State private var address = ""
    @State private var password = ""
    @State private var totpCode = ""

    var body: some View {
        NavigationStack {
            Form {
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
                                totpCode: totpCode.isEmpty ? nil : totpCode
                            )
                        }
                    } label: {
                        if session.state == .signingIn {
                            ProgressView()
                        } else {
                            Text("Sign in")
                        }
                    }
                    .disabled(address.isEmpty || password.isEmpty || session.state == .signingIn)
                }
            }
            .navigationTitle("Mailrs")
        }
    }
}
