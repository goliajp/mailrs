import SwiftUI

/// Mailboxes on this server: who exists, and their quota.
///
/// Backend: `crates/webapi/src/handlers/admin_directory.rs`, the same
/// `{items: […]}` envelope the alias list uses.
struct AccountsView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var accounts: [Wire.Account] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var adding = false
    @State private var pendingDelete: Wire.Account?

    @State private var address = ""
    @State private var displayName = ""
    @State private var password = ""

    private var byDomain: [(domain: String, accounts: [Wire.Account])] {
        Dictionary(grouping: accounts) { account in
            guard account.domain.isEmpty else { return account.domain }
            return AliasRule.domain(of: account.address)
        }
        .map { (domain: $0.key, accounts: $0.value.sorted { $0.address < $1.address }) }
        .sorted { $0.domain < $1.domain }
    }

    var body: some View {
        NavigationStack {
            Group {
                if loading, accounts.isEmpty {
                    ProgressView()
                } else if let failure, accounts.isEmpty {
                    ContentUnavailableView("Could not load accounts",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(failure))
                } else if accounts.isEmpty {
                    ContentUnavailableView("No accounts", systemImage: "person.2")
                } else {
                    List {
                        ForEach(byDomain, id: \.domain) { group in
                            Section(group.domain) {
                                ForEach(group.accounts) { account in
                                    AccountRow(account: account)
                                        .swipeActions {
                                            Button(role: .destructive) {
                                                pendingDelete = account
                                            } label: {
                                                Label("Delete", systemImage: "trash")
                                            }
                                        }
                                }
                            }
                        }
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("Accounts")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        adding = true
                    } label: {
                        Label("Add account", systemImage: "plus")
                    }
                }
            }
            .sheet(isPresented: $adding) { addSheet }
            .alert("Delete account?", isPresented: deleteBinding, presenting: pendingDelete) { account in
                Button("Delete", role: .destructive) {
                    Task { await delete(account) }
                    pendingDelete = nil
                }
                Button("Cancel", role: .cancel) { pendingDelete = nil }
            } message: { account in
                Text(verbatim: account.address)
            }
            .task { await load() }
        }
    }

    private var deleteBinding: Binding<Bool> {
        Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } })
    }

    private var addSheet: some View {
        NavigationStack {
            Form {
                Section("Address") {
                    TextField("someone@golia.jp", text: $address)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.emailAddress)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("account-address")
                }
                Section("Display name") {
                    TextField("Their name", text: $displayName)
                        .accessibilityIdentifier("account-name")
                }
                Section {
                    SecureField("Password", text: $password)
                        .accessibilityIdentifier("account-password")
                } footer: {
                    // Said plainly: this screen is the only place the
                    // password exists, and it is gone when the sheet is.
                    Text("Sent once and hashed by the server. It is not stored on this device.")
                }
                if let failure {
                    Section { Text(failure).foregroundStyle(.red) }
                }
            }
            .navigationTitle("New account")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { discard() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { Task { await add() } }
                        .disabled(!AddressList.isSendable(address) || password.isEmpty)
                }
            }
        }
    }

    private func load() async {
        loading = true
        do {
            accounts = try await session.accounts()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    private func add() async {
        do {
            try await session.addAccount(
                address: address.trimmingCharacters(in: .whitespaces),
                displayName: displayName,
                password: password
            )
            discard()
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }

    /// Closing the sheet clears the password, always. A compose sheet
    /// saves what you typed; this one must not, and the difference is
    /// worth a function rather than three assignments at each exit.
    private func discard() {
        address = ""
        displayName = ""
        password = ""
        adding = false
    }

    private func delete(_ account: Wire.Account) async {
        do {
            try await session.deleteAccount(address: account.address)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}

private struct AccountRow: View {
    let account: Wire.Account

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                SenderAvatar(sender: account.address, size: 28)
                ValueOrPlaceholder(value: account.displayName,
                                   placeholder: "\(account.address)")
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                if !account.active {
                    Text("Inactive")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color(.tertiarySystemFill), in: Capsule())
                }
                Spacer(minLength: 4)
                if account.quotaBytes > 0 {
                    Text(account.quotaBytes.formatted(.byteCount(style: .file)))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            if !account.displayName.isEmpty {
                Text(account.address)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 2)
    }
}
