import SwiftUI

/// Mailboxes somewhere else.
///
/// Adding one is meant to be an address and a secret. The address is
/// enough to know where the servers are; what it cannot know is the
/// secret, and for half the providers the thing to type is not the
/// login password at all but a code generated in their web UI. So the
/// form asks for the address first, looks the provider up, and only
/// then shows a secret field — labelled with the provider's own word
/// for it and with a link to the page that makes one.
struct MailAccountsView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.theme) private var theme
    @State private var model = MailAccountsModel()
    @FocusState private var focus: Field?

    private enum Field { case address, secret, name }

    var body: some View {
        NavigationStack {
            List {
                Section("Connected") {
                    if model.accounts.isEmpty {
                        Text("No other mailboxes yet.")
                            .font(.footnote)
                            .foregroundStyle(theme.fgMuted)
                    }
                    ForEach(model.accounts) { account in
                        row(account)
                    }
                }
                Section("Add a mailbox") {
                    TextField("you@example.com", text: $model.draft.address)
                        .textContentType(.emailAddress)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .focused($focus, equals: .address)
                        .accessibilityIdentifier("account.address")

                    if model.draft.addressLooksComplete {
                        providerNote
                        SecureField(model.draft.secretLabel, text: $model.draft.secret)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .focused($focus, equals: .secret)
                            .accessibilityIdentifier("account.secret")
                        TextField("Name it (optional)", text: $model.draft.name)
                            .focused($focus, equals: .name)
                            .accessibilityIdentifier("account.name")

                        // A disclosure, not a setting: it reveals
                        // fields rather than storing a preference, and
                        // it says which way it goes.
                        Button(model.draft.manual
                            ? "Discover the servers for me"
                            : "Enter the server settings myself") {
                            model.draft.manual.toggle()
                            if model.draft.manual { prefillManual() }
                        }
                        .font(.footnote)
                        .accessibilityIdentifier("account.manual")

                        if model.draft.manual { manualFields }

                        Button(model.busy ? "Checking…" : "Add") {
                            Task { await model.add() }
                        }
                        .disabled(model.busy || model.draft.secret.isEmpty)
                        .accessibilityIdentifier("account.add")
                    }
                }
                if let failure = model.failure {
                    Section {
                        Text(failure)
                            .font(.footnote)
                            .foregroundStyle(theme.danger)
                            .accessibilityIdentifier("account.failure")
                    }
                }
            }
            .navigationTitle("Mailboxes")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .task { model.load() }
        }
    }

    /// What this provider wants, said before anybody types it.
    @ViewBuilder private var providerNote: some View {
        if let p = model.draft.provider {
            if p.auth == .oauth2 {
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(p.label) does not accept a password for mail apps.")
                    Text("Sign in with \(p.label) is not built yet — a mailbox that takes an app password works today.")
                }
                .font(.caption)
                .foregroundStyle(theme.fgMuted)
                .accessibilityIdentifier("account.oauthUnavailable")
            } else if let help = p.secretHelp {
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(p.label) wants a \(help.what), not your login password.")
                    if let url = URL(string: help.url) {
                        Link("Get one", destination: url).font(.caption)
                    }
                }
                .font(.caption)
                .foregroundStyle(theme.fgMuted)
            }
        } else {
            // Shown rather than described. Saying "the usual names are
            // filled in below" while the boxes are shut is a sentence
            // about something the person cannot see — and if the guess
            // is wrong, they find out thirty seconds later from a
            // connection failure instead of now, from reading it.
            let guess = MailAccount.make(address: model.draft.address)
            VStack(alignment: .leading, spacing: 2) {
                Text("No preset for this domain. This will try:")
                Text("\(guess.imapHost):\(String(guess.imapPort)) and \(guess.smtpHost):\(String(guess.smtpPort))")
                    .foregroundStyle(theme.fgSecondary)
                Text("Open the settings below if that is not right.")
            }
            .font(.caption)
            .foregroundStyle(theme.fgMuted)
            .accessibilityIdentifier("account.noPreset")
        }
    }

    /// The boxes, when the guess is wrong or there is nothing to guess.
    @ViewBuilder private var manualFields: some View {
        // Offered only here: a preset knows its own answer, and asking
        // somebody to pick a protocol for Gmail is asking a question
        // already on file.
        Picker("Protocol", selection: $model.draft.incoming) {
            // Protocol names, not sentences: `verbatim` is the
            // spelling that says so, and translating them would be
            // wrong in every language.
            Text(verbatim: "IMAP").tag(Incoming.imap)
            Text(verbatim: "POP3").tag(Incoming.pop3)
            Text(verbatim: "JMAP").tag(Incoming.jmap)
        }
        .pickerStyle(.segmented)
        .accessibilityIdentifier("account.kind")
        .onChange(of: model.draft.incoming) { _, _ in
            // The port follows the protocol, because the two are not
            // independent: 993 in a POP3 form is a number somebody has
            // to already know is wrong.
            model.draft.imapPort = ""
            prefillManual()
        }
        TextField("Incoming server", text: $model.draft.imapHost)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .accessibilityIdentifier("account.incoming.host")
        TextField("Port", text: $model.draft.imapPort)
            .keyboardType(.numberPad)
            .accessibilityIdentifier("account.incoming.port")
        // JMAP submits over the same API, so there is no second server
        // to name. A box for one would be a box somebody fills in with
        // the first server's name to get past the form.
        if model.draft.incoming != .jmap {
            TextField("Outgoing server", text: $model.draft.smtpHost)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .accessibilityIdentifier("account.outgoing.host")
            TextField("Port", text: $model.draft.smtpPort)
                .keyboardType(.numberPad)
                .accessibilityIdentifier("account.outgoing.port")
        }
        TextField("Login name, if it is not the address", text: $model.draft.login)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .accessibilityIdentifier("account.login")
    }

    /// Filled in from what is known, so the boxes open with the usual
    /// answers rather than empty. An empty form is a form somebody has
    /// to research; a filled one is a form they correct.
    private func prefillManual() {
        let a = MailAccount.make(address: model.draft.address)
        if model.draft.imapHost.isEmpty { model.draft.imapHost = a.imapHost }
        if model.draft.imapPort.isEmpty {
            model.draft.imapPort = Self.defaultPort(for: model.draft.incoming, imap: a.imapPort)
        }
        if model.draft.smtpHost.isEmpty { model.draft.smtpHost = a.smtpHost }
        if model.draft.smtpPort.isEmpty { model.draft.smtpPort = String(a.smtpPort) }
    }

    /// The port a protocol usually listens on.
    private static func defaultPort(for kind: Incoming, imap: UInt16) -> String {
        switch kind {
        case .pop3: return "995"
        case .jmap: return "443"
        case .imap: return String(imap)
        }
    }

    private func row(_ account: MailAccount) -> some View {
        HStack(spacing: 10) {
            Circle()
                .fill(Color(hex: AccountColour.forId(account.id)))
                .frame(width: 10, height: 10)
            VStack(alignment: .leading, spacing: 1) {
                Text(account.title)
                if let second = accountSubtitle(account) {
                    Text(second).font(.caption).foregroundStyle(theme.fgMuted)
                }
            }
            Spacer()
            Text(account.provider == "custom" ? "IMAP" : account.provider.uppercased())
                .font(.caption2)
                .foregroundStyle(theme.fgMuted)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("account.\(account.id)")
        .swipeActions {
            Button("Remove", role: .destructive) { model.remove(account) }
        }
    }

    /// The second line, or nothing.
    ///
    /// An account with no name of its own falls back to its address on
    /// the first line, so repeating the address underneath says
    /// nothing and reads as a rendering fault.
    private func accountSubtitle(_ a: MailAccount) -> String? {
        (a.displayName.isEmpty || a.displayName == a.address) ? nil : a.address
    }
}
