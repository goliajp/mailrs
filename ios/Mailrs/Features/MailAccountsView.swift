import SwiftUI

/// Mailboxes somewhere else.
///
/// Connecting one is an address and a secret. The address is enough to
/// know where Gmail's servers are; what it cannot know is that half
/// the providers refuse the login password and want a code generated
/// in their web UI instead — so the screen looks the domain up as the
/// address is typed, and labels the secret field with the provider's
/// own word for it.
struct MailAccountsView: View {
    @Environment(Session.self) private var session
    @Environment(\.theme) private var theme
    @Environment(\.dismiss) private var dismiss

    @State private var accounts: [Wire.ExternalAccount] = []
    @State private var email = ""
    @State private var secret = ""
    @State private var name = ""
    @State private var settings: Wire.AccountSettings?
    @State private var failure: String?
    @State private var busy = false

    /// A partial address is not a domain; asking about "s", "so", "som"
    /// is three requests that cannot answer anything.
    private var complete: Bool {
        let parts = email.split(separator: "@")
        return parts.count == 2 && parts[1].contains(".")
    }

    private var secretLabel: String {
        settings?.preset?.secretHelp?.what ?? "Password"
    }

    var body: some View {
        NavigationStack {
            List {
                Section("Connected") {
                    if accounts.isEmpty {
                        Text("No other accounts connected yet.")
                            .font(.footnote)
                            .foregroundStyle(theme.fgMuted)
                    }
                    ForEach(accounts) { a in
                        row(a)
                    }
                }
                Section("Connect an account") {
                    TextField("you@gmail.com", text: $email)
                        .textContentType(.emailAddress)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("account.email")
                    if complete {
                        providerNote
                        if settings?.preset?.auth != "oauth2" {
                            SecureField(secretLabel, text: $secret)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                                .accessibilityIdentifier("account.secret")
                            TextField("Name it (optional)", text: $name)
                                .accessibilityIdentifier("account.name")
                            Button(busy ? "Connecting…" : "Connect", action: connect)
                                .disabled(busy || secret.isEmpty)
                                .accessibilityIdentifier("account.connect")
                        }
                    }
                }
                if let failure {
                    Text(failure)
                        .font(.footnote)
                        .foregroundStyle(theme.danger)
                        .accessibilityIdentifier("account.failure")
                }
            }
            .navigationTitle("Mail accounts")
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() } } }
            .task { await load() }
            .task(id: complete ? email : "") { await lookUp() }
        }
    }

    @ViewBuilder private var providerNote: some View {
        if let p = settings?.preset {
            if p.auth == "oauth2" {
                Text("\(p.label) does not accept a password for mail apps — connecting it opens a sign-in page.",
                     comment: "%@ is the provider's name")
                    .font(.caption)
                    .foregroundStyle(theme.fgMuted)
            } else if let help = p.secretHelp {
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(p.label) wants a \(help.what), not your login password.",
                         comment: "provider name, then the provider's own word for the secret")
                    if let url = URL(string: help.url) {
                        Link("Get one", destination: url).font(.caption)
                    }
                }
                .font(.caption)
                .foregroundStyle(theme.fgMuted)
            }
        } else if settings?.known == false {
            Text("Its server settings will be discovered from DNS when the account is added.")
                .font(.caption)
                .foregroundStyle(theme.fgMuted)
        }
    }

    private func row(_ a: Wire.ExternalAccount) -> some View {
        HStack(spacing: 10) {
            Circle()
                .fill(Color(hex: a.colour ?? "#6b7280"))
                .frame(width: 10, height: 10)
            VStack(alignment: .leading, spacing: 1) {
                Text(a.displayName.isEmpty ? a.email : a.displayName)
                Text(a.email).font(.caption).foregroundStyle(theme.fgMuted)
            }
            Spacer()
            // A broken account has to say so where it was added.
            // Silence means somebody believes they are seeing all their
            // mail when they are not.
            if let trouble = a.trouble {
                Text(trouble)
                    .font(.caption2)
                    .foregroundStyle(a.state == "needs_auth" ? theme.warning : theme.danger)
            }
        }
        .accessibilityIdentifier("account.\(a.id)")
        .swipeActions {
            Button("Remove", role: .destructive) { Task { await remove(a) } }
        }
    }

    private func load() async {
        do { accounts = try await session.client?.externalAccounts() ?? [] } catch { accounts = [] }
    }

    private func lookUp() async {
        guard complete else {
            settings = nil
            return
        }
        settings = try? await session.client?.accountSettings(for: email)
    }

    private func connect() {
        Task {
            busy = true
            failure = nil
            do {
                try await session.client?.connectAccount(email: email, secret: secret, name: name)
                email = ""
                secret = ""
                name = ""
                settings = nil
                await load()
            } catch {
                failure = error.localizedDescription
            }
            busy = false
        }
    }

    private func remove(_ a: Wire.ExternalAccount) async {
        do {
            try await session.client?.disconnectAccount(id: a.id)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}
