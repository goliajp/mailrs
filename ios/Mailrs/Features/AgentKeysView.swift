import SwiftUI

/// The keys that let a program act as this account.
///
/// Backend: `crates/webapi/src/handlers/apps_keys.rs`. The list is an
/// `{items: […]}` envelope; creation answers `{id, secret}` and the
/// server keeps only the first eight characters of that secret. So the
/// whole screen is shaped by one fact: **there is exactly one moment in
/// which the secret exists**, and if it is missed the only remedy is to
/// revoke the key and make another.
struct AgentKeysView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var keys: [Wire.AgentKey] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var adding = false
    @State private var name = ""
    @State private var scopes: Set<String> = []
    @State private var catalogue: [String] = []
    /// Held from the moment of creation until the reader dismisses it.
    @State private var madeSecret: String?
    @State private var pendingRevoke: Wire.AgentKey?

    var body: some View {
        NavigationStack {
            Group {
                if loading, keys.isEmpty {
                    ProgressView()
                } else if let failure, keys.isEmpty {
                    ContentUnavailableView("Could not load keys",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(verbatim: failure))
                } else if keys.isEmpty {
                    ContentUnavailableView(
                        "No API keys", systemImage: "key",
                        description: Text("A key lets a program read and send mail as you.")
                    )
                } else {
                    List {
                        ForEach(keys) { key in
                            AgentKeyRow(key: key)
                                .swipeActions {
                                    Button(role: .destructive) {
                                        pendingRevoke = key
                                    } label: {
                                        Label("Revoke", systemImage: "trash")
                                    }
                                }
                        }
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("API keys")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        adding = true
                    } label: {
                        Label("New key", systemImage: "plus")
                    }
                }
            }
            .sheet(isPresented: $adding) { addSheet }
            .sheet(item: secretBinding) { made in
                SecretOnceSheet(secret: made.secret)
            }
            .alert("Revoke this key?", isPresented: revokeBinding, presenting: pendingRevoke) { key in
                Button("Revoke", role: .destructive) {
                    let id = key.id
                    Task { await revoke(id) }
                    pendingRevoke = nil
                }
                Button("Cancel", role: .cancel) { pendingRevoke = nil }
            } message: { key in
                // Named, and with what stops working: revoking is the
                // one action here that breaks something already running.
                Text("\(key.name) stops working immediately.")
            }
            .task { await load() }
        }
    }

    /// `.sheet(item:)` wants something identifiable; the secret is a
    /// string, and a string is not.
    private struct MadeKey: Identifiable {
        let secret: String
        var id: String { secret }
    }

    private var secretBinding: Binding<MadeKey?> {
        Binding(
            get: { madeSecret.map(MadeKey.init) },
            set: { if $0 == nil { madeSecret = nil } }
        )
    }

    private var revokeBinding: Binding<Bool> {
        Binding(get: { pendingRevoke != nil }, set: { if !$0 { pendingRevoke = nil } })
    }

    private var addSheet: some View {
        NavigationStack {
            Form {
                Section("Name") {
                    TextField("Scheduler", text: $name)
                        .accessibilityIdentifier("key-name")
                }
                Section("What it may do") {
                    // The same catalogue the permission groups use,
                    // fetched rather than copied: a second list of
                    // scope strings is a second thing to keep in step.
                    ForEach(catalogue, id: \.self) { scope in
                        Button {
                            scopes.formSymmetricDifference([scope])
                        } label: {
                            HStack {
                                Text(verbatim: scope)
                                    .foregroundStyle(.primary)
                                Spacer()
                                if scopes.contains(scope) {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(Color.accentColor)
                                }
                            }
                        }
                    }
                }
                if let failure {
                    Section { Text(verbatim: failure).foregroundStyle(.red) }
                }
            }
            .navigationTitle("New key")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { adding = false }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") { Task { await create() } }
                        // A key that may do nothing is not a key.
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || scopes.isEmpty)
                }
            }
        }
    }

    private func load() async {
        loading = true
        do {
            keys = try await session.agentKeys()
            catalogue = try await session.permissionCatalogue()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    private func create() async {
        do {
            let made = try await session.createAgentKey(
                name: name.trimmingCharacters(in: .whitespaces), scopes: Array(scopes).sorted()
            )
            adding = false
            name = ""
            scopes = []
            madeSecret = made.secret
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }

    private func revoke(_ id: Int64) async {
        do {
            try await session.deleteAgentKey(id: id)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}

private struct AgentKeyRow: View {
    let key: Wire.AgentKey

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(verbatim: key.name)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                Spacer(minLength: 4)
                RowDateText(epochSeconds: key.createdAt)
            }
            // The prefix is the only handle a key has once it is made,
            // so it is shown the way it will be recognised: monospaced,
            // and ending in an ellipsis rather than pretending to be
            // the whole thing.
            Text(verbatim: "\(key.prefix)…")
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
            if !key.scopes.isEmpty {
                Text(verbatim: key.scopes.joined(separator: " · "))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 2)
    }
}

/// The one moment the secret exists.
///
/// A sheet rather than a row in the list, because the list is where it
/// will *not* be afterwards. Copy is the primary action and the text is
/// selectable as well; the dismissal says plainly that this is the end
/// of it.
private struct SecretOnceSheet: View {
    let secret: String
    @Environment(\.dismiss) private var dismiss
    @State private var copied = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 16) {
                Image(systemName: "key.horizontal.fill")
                    .font(.system(size: 40))
                    .foregroundStyle(Color.accentColor)
                Text("Copy this key now")
                    .font(.headline)
                Text("It is not stored and cannot be shown again. If you lose it, revoke the key and make another.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                Text(verbatim: secret)
                    .font(.callout.monospaced())
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .background(Color(.secondarySystemGroupedBackground),
                                in: RoundedRectangle(cornerRadius: 10))
                Button {
                    UIPasteboard.general.string = secret
                    withAnimation { copied = true }
                } label: {
                    Label(copiedLabel, systemImage: copiedIcon)
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("copy-secret")
                Spacer()
            }
            .padding(20)
            .navigationTitle("New key")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("secret-done")
                }
            }
        }
    }

    private var copiedLabel: LocalizedStringKey {
        if copied { return "Copied" }
        return "Copy"
    }

    private var copiedIcon: String {
        if copied { return "checkmark" }
        return "doc.on.doc"
    }
}
