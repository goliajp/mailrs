import SwiftUI

/// Email groups: one address that delivers to many people, where an
/// alias delivers to one. Together they answer the whole of "where
/// does mail for this address go".
struct EmailGroupsView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var groups: [Wire.EmailGroup] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var adding = false
    @State private var address = ""
    @State private var name = ""
    @State private var pendingDelete: Wire.EmailGroup?

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
                        "No groups", systemImage: "person.3",
                        description: Text("A group delivers one address to several people.")
                    )
                } else {
                    List(groups) { group in
                        NavigationLink {
                            EmailGroupDetailView(group: group)
                        } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                ValueOrPlaceholder(value: group.name, placeholder: "\(group.address)")
                                    .font(.subheadline.weight(.medium))
                                    .lineLimit(1)
                                Text(group.address)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                        .swipeActions {
                            Button(role: .destructive) {
                                pendingDelete = group
                            } label: {
                                Label("Delete", systemImage: "trash")
                            }
                        }
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("Groups")
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
            .sheet(isPresented: $adding) { addSheet }
            .alert("Delete group?", isPresented: deleteBinding, presenting: pendingDelete) { group in
                Button("Delete", role: .destructive) {
                    Task { await delete(group) }
                    pendingDelete = nil
                }
                Button("Cancel", role: .cancel) { pendingDelete = nil }
            } message: { group in
                Text("\(group.address) — its members keep their own mailboxes.")
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
                    TextField("team@golia.jp", text: $address)
                        .neverCapitalised()
                        .mailKeyboard()
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("group-address")
                }
                Section("Name") {
                    TextField("Team", text: $name)
                        .accessibilityIdentifier("group-name")
                }
            }
            .navigationTitle("New group")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { adding = false }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { Task { await add() } }
                        .disabled(!AddressList.isSendable(address))
                }
            }
        }
    }

    private func load() async {
        loading = true
        do {
            groups = try await session.emailGroups()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    private func add() async {
        do {
            try await session.createEmailGroup(
                address: address.trimmingCharacters(in: .whitespaces),
                name: name,
                description: ""
            )
            address = ""
            name = ""
            adding = false
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }

    private func delete(_ group: Wire.EmailGroup) async {
        do {
            try await session.deleteEmailGroup(id: group.id)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}

/// Who a group delivers to.
struct EmailGroupDetailView: View {
    let group: Wire.EmailGroup
    @Environment(Session.self) private var session

    @State private var members: [String] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var adding = false
    @State private var newMember = ""

    private var groupTitle: String {
        if group.name.isEmpty { return group.address }
        return group.name
    }

    var body: some View {
        Group {
            if loading, members.isEmpty {
                ProgressView()
            } else if members.isEmpty {
                ContentUnavailableView("No members", systemImage: "person.badge.plus",
                                       description: Text("Mail to this address goes nowhere."))
            } else {
                List {
                    Section(group.address) {
                        ForEach(members, id: \.self) { member in
                            HStack(spacing: 10) {
                                SenderAvatar(sender: member, size: 28)
                                Text(SenderName.extractName(member))
                                    .font(.subheadline)
                                    .lineLimit(1)
                                    .layoutPriority(1)
                                Spacer()
                                Text(SenderName.extractEmail(member))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                            .swipeActions {
                                Button(role: .destructive) {
                                    Task { await remove(member) }
                                } label: {
                                    Label("Delete", systemImage: "trash")
                                }
                            }
                        }
                    }
                }
                .refreshable { await load() }
            }
        }
        .navigationTitle(groupTitle)
        .inlineTitle()
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button {
                    adding = true
                } label: {
                    Label("Add member", systemImage: "plus")
                }
            }
        }
        .alert("Add member", isPresented: $adding) {
            TextField("someone@golia.jp", text: $newMember)
                .neverCapitalised()
                .autocorrectionDisabled()
            Button("Add") { Task { await add() } }
            Button("Cancel", role: .cancel) { newMember = "" }
        }
        .task { await load() }
    }

    private func load() async {
        loading = true
        do {
            members = try await session.emailGroupMembers(id: group.id)
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    private func add() async {
        let wanted = SenderName.extractEmail(newMember.trimmingCharacters(in: .whitespaces))
        newMember = ""
        guard AddressList.isSendable(wanted) else { return }
        do {
            try await session.addEmailGroupMember(id: group.id, address: wanted)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }

    private func remove(_ member: String) async {
        do {
            try await session.removeEmailGroupMember(id: group.id, address: member)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}
