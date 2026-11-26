import SwiftUI

/// The sign-in screen's server line: which server, and a way to change it.
struct ServerChoiceRow: View {
    @Environment(Session.self) private var session

    var body: some View {
        let address = session.baseURL.absoluteString
        let known = session.servers.list.entry(at: address)
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: known?.name ?? Self.shown(address))
                    .foregroundStyle(Color.primary)
                if let known {
                    Text(verbatim: ServerAddress.display(known.address))
                        .font(.footnote)
                        // `Color.secondary`: inside a button the hierarchical style takes the tint
                        .foregroundStyle(Color.secondary)
                }
            }
            Spacer(minLength: 0)
            Image(systemName: "chevron.up.chevron.down")
                .font(.footnote.weight(.semibold))
                .foregroundStyle(Color.tertiaryText)
                .accessibilityHidden(true)
        }
        .accessibilityElement(children: .combine)
    }

    private static func shown(_ address: String) -> String {
        ServerAddress.normalise(address).map(ServerAddress.display) ?? address
    }
}

/// The saved servers: choose one, add one, change or remove one.
struct ServerPickerSheet: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss
    @State private var editing: ServerEditSheet.Target?

    private var servers: ServerStore { session.servers }

    private var selected: SavedServer? {
        servers.list.entry(at: session.baseURL.absoluteString)
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(servers.list.visible) { server in
                        row(server)
                    }
                } footer: {
                    Text("The server you last signed in to is chosen for you next time.")
                }
                Section {
                    Button {
                        editing = .new
                    } label: {
                        Label("Add server", systemImage: "plus")
                    }
                    .accessibilityIdentifier("server.add")
                    if servers.list.canRestoreBuiltIn {
                        Button {
                            servers.update { $0.restoreBuiltIn() }
                        } label: {
                            Label("Show \(ServerList.builtIn.name) again", systemImage: "arrow.uturn.backward")
                        }
                        .accessibilityIdentifier("server.restore")
                    }
                }
            }
            .navigationTitle("Servers")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("server.done")
                }
            }
            .sheet(item: $editing) { target in
                ServerEditSheet(target: target)
            }
        }
    }

    @ViewBuilder
    private func row(_ server: SavedServer) -> some View {
        let isSelected = server.id == selected?.id
        HStack(spacing: 12) {
            Button {
                session.use(server)
                dismiss()
            } label: {
                HStack(spacing: 12) {
                    // the column is kept when empty, so the names line up
                    ZStack {
                        if isSelected {
                            Image(systemName: "checkmark")
                                .font(.body.weight(.semibold))
                                .foregroundStyle(Color.accentColor)
                        }
                    }
                    .frame(width: 20)
                    .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(verbatim: server.name)
                            .foregroundStyle(Color.primary)
                        Text(verbatim: ServerAddress.display(server.address))
                            .font(.footnote)
                            // `Color.secondary`: inside a button the hierarchical style takes the tint
                        .foregroundStyle(Color.secondary)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .accessibilityIdentifier("server.row.\(server.name)")
            .accessibilityAddTraits(Self.traits(selected: isSelected))

            if server.id == ServerList.builtIn.id {
                if servers.list.canHideBuiltIn {
                    Button {
                        hide(server)
                    } label: {
                        Image(systemName: "eye.slash")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Hide \(server.name)")
                    .accessibilityIdentifier("server.hideBuiltIn")
                }
            } else {
                Button {
                    editing = .existing(server)
                } label: {
                    Image(systemName: "pencil")
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Edit \(server.name)")
                .accessibilityIdentifier("server.edit.\(server.name)")
            }
        }
    }

    private static func traits(selected: Bool) -> AccessibilityTraits {
        if selected { return .isSelected }
        return []
    }

    private func hide(_ server: SavedServer) {
        servers.update { $0.remove(id: server.id) }
        session.keepToListedServer(leaving: server)
    }
}

extension Session {
    /// Point the sign-in at a saved server.
    func use(_ server: SavedServer) {
        guard let url = URL(string: server.address) else { return }
        baseURL = url
    }

    /// After `gone` left the list: if the sign-in was pointing at it,
    /// point it at the one the list now starts with.
    func keepToListedServer(leaving gone: SavedServer) {
        guard baseURL.absoluteString == gone.address
            || ServerAddress.normalise(baseURL.absoluteString) == gone.address
        else { return }
        use(servers.list.preselected)
    }
}
