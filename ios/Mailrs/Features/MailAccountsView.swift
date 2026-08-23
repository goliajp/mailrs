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
    /// Shut unless somebody opens it: a form that opens with eight
    /// empty boxes teaches everybody that connecting mail is hard.
    @State private var manual = false
    @State private var incoming = ManualEndpoint(proto: "imap")
    @State private var outgoing = ManualEndpoint(proto: "smtp")
    @State private var login = ""

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
                            // A disclosure, not a setting: it reveals
                            // fields rather than storing a preference,
                            // and it says which way it goes — the same
                            // control the web and Android use.
                            Button(manual
                                ? "Discover the servers for me"
                                : "Enter the server settings myself") {
                                manual.toggle()
                            }
                            .font(.footnote)
                            .accessibilityIdentifier("account.manual")
                            if manual {
                                manualFields
                            }
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

    /// The boxes, when autodiscovery cannot reach a server.
    @ViewBuilder private var manualFields: some View {
        endpointFields("Incoming", $incoming, protocols: ["imap", "pop3", "jmap"])
        endpointFields("Outgoing", $outgoing, protocols: ["smtp"])
        TextField("Login name, if it is not the address", text: $login)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .accessibilityIdentifier("account.login")
    }

    @ViewBuilder private func endpointFields(
        _ label: String, _ e: Binding<ManualEndpoint>, protocols: [String]
    ) -> some View {
        TextField("\(label) server", text: e.host)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .accessibilityIdentifier("account.\(label.lowercased()).host")
        TextField("Port", text: e.port)
            .keyboardType(.numberPad)
            .accessibilityIdentifier("account.\(label.lowercased()).port")
        if protocols.count > 1 {
            Picker("\(label) protocol", selection: e.proto) {
                ForEach(protocols, id: \.self) { Text($0.uppercased()).tag($0) }
            }
            .accessibilityIdentifier("account.\(label.lowercased()).protocol")
        }
        Picker("\(label) encryption", selection: e.tls) {
            Text("TLS from the first byte").tag("implicit")
            Text("STARTTLS").tag("starttls")
            Text("None").tag("none")
        }
        .accessibilityIdentifier("account.\(label.lowercased()).tls")
    }

    @ViewBuilder private var providerNote: some View {
        if let p = settings?.preset {
            if p.auth == "oauth2" {
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(p.label) does not accept a password for mail apps.",
                         comment: "%@ is the provider's name")
                    // Said here rather than discovered at the end of a
                    // sign-in that could not have finished. When this
                    // deployment registers an application, this
                    // becomes the hand-off to the provider's own page.
                    Text("This server cannot connect \(p.label) accounts yet.",
                         comment: "%@ is the provider's name")
                }
                .font(.caption)
                .foregroundStyle(theme.fgMuted)
                .accessibilityIdentifier("account.oauthUnavailable")
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
                // Only when it says something the line above did not:
                // an account with no name showed its address twice.
                if let second = accountSubtitle(displayName: a.displayName, email: a.email) {
                    Text(second).font(.caption).foregroundStyle(theme.fgMuted)
                }
                // The reason, on the screen somebody actually reads.
                // The web had it in a hover tooltip and the phones had
                // it nowhere.
                if a.state != "ok", let why = a.lastError, !why.isEmpty {
                    Text(why.count > 200 ? String(why.prefix(200)) + "…" : why)
                        .font(.caption2)
                        .foregroundStyle(theme.fgMuted)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("account.why.\(a.id)")
                }
                // Work, not a fault: a re-read after the server
                // renumbered a folder takes as long as a mailbox is
                // big, and silence for that long reads as a stall.
                if let note = a.progress, !note.isEmpty {
                    Text(note)
                        .font(.caption2)
                        .foregroundStyle(theme.fgMuted)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("account.progress.\(a.id)")
                }
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
        // `.contain` so the reason and the progress note stay their own
        // elements. An identifier on a container makes the container
        // the element and hides what is inside it — VoiceOver reads one
        // blob, and nothing can point at the line that says why this
        // account stopped.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("account.\(a.id)")
        .swipeActions {
            Button("Remove", role: .destructive) { Task { await remove(a) } }
        }
    }

    /// A failure here is not an empty list.
    ///
    /// It said nothing and showed nothing, so a decoding fault — the
    /// client read this response as an array for as long as it
    /// existed, and it is an object — was indistinguishable from
    /// having connected no mailboxes.
    private func load() async {
        do {
            accounts = try await session.client?.externalAccounts() ?? []
            failure = nil
        } catch {
            accounts = []
            failure = error.localizedDescription
        }
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
                var servers: [String: Any]?
                if manual {
                    guard let s = wireEndpoints(incoming: incoming, outgoing: outgoing) else {
                        failure = String(
                            localized: "Both servers need a name and a port",
                            comment: "manual server settings are incomplete")
                        busy = false
                        return
                    }
                    servers = s
                }
                try await session.client?.connectAccount(
                    email: email, secret: secret, name: name,
                    servers: servers, login: manual ? login : "")
                email = ""
                secret = ""
                name = ""
                login = ""
                manual = false
                incoming = ManualEndpoint(proto: "imap")
                outgoing = ManualEndpoint(proto: "smtp")
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
