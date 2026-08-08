import SwiftUI

/// Alias management: which addresses route where.
///
/// Backend: `crates/webapi/src/handlers/admin_directory.rs`. The list
/// arrives in an `{items: […]}` envelope — the conversation list does
/// not, and holding both shapes in one app is why the wire type says
/// so out loud.
struct AliasesView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var aliases: [Wire.Alias] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var adding = false
    @State private var source = ""
    @State private var target = ""
    @State private var pendingDelete: Wire.Alias?

    /// Grouped by domain, because an operator thinks in domains: every
    /// address on golia.jp, then every address on golia.ai.
    private var byDomain: [(domain: String, aliases: [Wire.Alias])] {
        Dictionary(grouping: aliases) { alias in
            guard alias.domain.isEmpty else { return alias.domain }
            return AliasRule.domain(of: alias.sourceAddress)
        }
        .map { (domain: $0.key, aliases: $0.value.sorted { $0.sourceAddress < $1.sourceAddress }) }
        .sorted { $0.domain < $1.domain }
    }

    var body: some View {
        NavigationStack {
            Group {
                if loading, aliases.isEmpty {
                    ProgressView()
                } else if let failure, aliases.isEmpty {
                    ContentUnavailableView("Could not load aliases",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(failure))
                } else if aliases.isEmpty {
                    ContentUnavailableView("No aliases", systemImage: "arrow.triangle.branch")
                } else {
                    List {
                        ForEach(byDomain, id: \.domain) { group in
                            Section(group.domain) {
                                ForEach(group.aliases) { alias in
                                    AliasRow(alias: alias)
                                        .swipeActions {
                                            Button(role: .destructive) {
                                                pendingDelete = alias
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
            .navigationTitle("Aliases")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        adding = true
                    } label: {
                        Label("Add alias", systemImage: "plus")
                    }
                }
            }
            .sheet(isPresented: $adding) { addSheet }
            .alert("Delete alias?", isPresented: deleteBinding, presenting: pendingDelete) { alias in
                Button("Delete", role: .destructive) {
                    Task { await delete(alias) }
                    pendingDelete = nil
                }
                Button("Cancel", role: .cancel) { pendingDelete = nil }
            } message: { alias in
                // Named rather than "this alias": a list of near-identical
                // addresses is exactly where a confirmation earns its keep.
                Text(verbatim: "\(alias.sourceAddress) → \(alias.targetAddress)")
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
                Section("Alias") {
                    TextField("sales@golia.jp", text: $source)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.emailAddress)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("alias-source")
                }
                Section("Delivers to") {
                    TextField("someone@golia.jp", text: $target)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.emailAddress)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("alias-target")
                }
                if let failure {
                    Section { Text(failure).foregroundStyle(.red) }
                }
            }
            .navigationTitle("New alias")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { adding = false }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { Task { await add() } }
                        .disabled(!AliasRule.isCreatable(source: source, target: target))
                }
            }
        }
    }

    private func load() async {
        loading = true
        do {
            aliases = try await session.aliases()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    private func add() async {
        let from = SenderName.extractEmail(source.trimmingCharacters(in: .whitespaces))
        let to = SenderName.extractEmail(target.trimmingCharacters(in: .whitespaces))
        do {
            try await session.addAlias(source: from, target: to)
            source = ""
            target = ""
            adding = false
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }

    private func delete(_ alias: Wire.Alias) async {
        do {
            try await session.deleteAlias(id: alias.id)
            // Not optimistic: an alias that looks deleted and still
            // routes mail is worse than a row that lingers a moment.
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}

private struct AliasRow: View {
    let alias: Wire.Alias

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Text(alias.sourceAddress)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                if !alias.active {
                    Text("Inactive")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color(.tertiarySystemFill), in: Capsule())
                }
            }
            HStack(spacing: 4) {
                Image(systemName: "arrow.turn.down.right")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Text(alias.targetAddress)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 2)
    }
}
