import SwiftUI

/// Who may do what.
///
/// A permission group, not an email group: this one decides who is
/// allowed, the other decides where mail goes. Backend:
/// `crates/webapi/src/handlers/groups.rs`.
struct PermissionGroupsView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var groups: [Wire.PermissionGroup] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var adding = false
    @State private var name = ""
    @State private var pendingDelete: Wire.PermissionGroup?

    var body: some View {
        NavigationStack {
            Group {
                if loading, groups.isEmpty {
                    ProgressView()
                } else if let failure, groups.isEmpty {
                    ContentUnavailableView("Could not load groups",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(failure))
                } else if groups.isEmpty {
                    ContentUnavailableView(
                        "No groups", systemImage: "lock.shield",
                        description: Text("A group grants its members the permissions you tick.")
                    )
                } else {
                    List(groups) { group in
                        NavigationLink {
                            PermissionGroupDetailView(group: group)
                        } label: {
                            GroupRow(group: group)
                        }
                        .swipeActions {
                            // Builtins have no delete: the server owns
                            // them, and offering the action would be
                            // offering a failure.
                            if !group.isBuiltin {
                                Button(role: .destructive) {
                                    pendingDelete = group
                                } label: {
                                    Label("Delete", systemImage: "trash")
                                }
                            }
                        }
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("Permissions")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        adding = true
                    } label: {
                        Label("Add group", systemImage: "plus")
                    }
                }
            }
            .alert("New group", isPresented: $adding) {
                TextField("Support", text: $name)
                Button("Add") { Task { await add() } }
                Button("Cancel", role: .cancel) { name = "" }
            }
            .alert("Delete group?", isPresented: deleteBinding, presenting: pendingDelete) { group in
                Button("Delete", role: .destructive) {
                    Task { await delete(group) }
                    pendingDelete = nil
                }
                Button("Cancel", role: .cancel) { pendingDelete = nil }
            } message: { group in
                // Not `verbatim`: this is the sentence telling someone
                // what deleting will do, and it was the one string on
                // the screen that stayed English in every language.
                Text("\(group.name) — its members lose every permission it granted.")
            }
            .task { await load() }
        }
    }

    private var deleteBinding: Binding<Bool> {
        Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } })
    }

    private func load() async {
        loading = true
        do {
            groups = try await session.permissionGroups()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    private func add() async {
        let wanted = name.trimmingCharacters(in: .whitespaces)
        name = ""
        guard !wanted.isEmpty else { return }
        do {
            try await session.createPermissionGroup(name: wanted, domain: nil, description: "")
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }

    private func delete(_ group: Wire.PermissionGroup) async {
        do {
            try await session.deletePermissionGroup(id: group.id)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}

private struct GroupRow: View {
    let group: Wire.PermissionGroup

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Text(verbatim: group.name)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                if group.isBuiltin {
                    Text("Built in")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color(.tertiarySystemFill), in: Capsule())
                }
            }
            if let domain = group.domain, !domain.isEmpty {
                Text(verbatim: domain)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                Text("Every domain")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 2)
    }
}

/// A group's members, and the permissions it grants.
struct PermissionGroupDetailView: View {
    let group: Wire.PermissionGroup
    @Environment(Session.self) private var session

    @State private var members: [String] = []
    @State private var granted: Set<String> = []
    @State private var catalogue: [String] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var saving = false

    var body: some View {
        List {
            Section("Permissions") {
                ForEach(catalogue, id: \.self) { permission in
                    Button {
                        Task { await toggle(permission) }
                    } label: {
                        HStack {
                            Text(verbatim: permission)
                                .font(.subheadline)
                                .foregroundStyle(.primary)
                                .lineLimit(1)
                            Spacer()
                            if granted.contains(permission) {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(Color.accentColor)
                            }
                        }
                    }
                    // Plain: eleven tinted rows read as eleven links to
                    // follow. This is a checklist, and the checkmark is
                    // the only thing that should be accented.
                    .buttonStyle(.plain)
                    .disabled(saving || group.isBuiltin)
                }
            }

            Section("Members") {
                if members.isEmpty {
                    Text("No members").foregroundStyle(.secondary)
                } else {
                    ForEach(members, id: \.self) { member in
                        HStack(spacing: 10) {
                            SenderAvatar(sender: member, size: 28)
                            Text(verbatim: member)
                                .font(.subheadline)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                    }
                }
            }

            if let failure {
                Section { Text(failure).foregroundStyle(.red) }
            }
        }
        .navigationTitle(Text(verbatim: group.name))
        .inlineTitle()
        .overlay {
            if loading, catalogue.isEmpty { ProgressView() }
        }
        .task { await load() }
    }

    private func load() async {
        loading = true
        do {
            catalogue = try await session.permissionCatalogue()
            granted = Set(try await session.groupPermissions(id: group.id))
            members = try await session.groupMembers(id: group.id)
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    /// The endpoint replaces rather than merges, so a toggle sends the
    /// whole set — sending one permission would grant that and revoke
    /// everything else.
    private func toggle(_ permission: String) async {
        var wanted = granted
        if wanted.contains(permission) {
            wanted.remove(permission)
        } else {
            wanted.insert(permission)
        }
        let previous = granted
        saving = true
        granted = wanted
        do {
            try await session.setGroupPermissions(id: group.id, permissions: Array(wanted).sorted())
        } catch {
            granted = previous
            failure = error.localizedDescription
        }
        saving = false
    }
}
