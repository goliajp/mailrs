import SwiftUI

/// Adding a server, or changing one the reader added: a name and an address.
struct ServerEditSheet: View {
    enum Target: Identifiable {
        case new
        case existing(SavedServer)

        var id: String {
            switch self {
            case .new: "new"
            case .existing(let server): server.id
            }
        }
    }

    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss
    let target: Target

    @State private var name = ""
    @State private var address = ""
    @State private var loaded = false

    private var existing: SavedServer? {
        if case .existing(let server) = target { return server }
        return nil
    }

    /// What is wrong with the address as typed, nil when it can be saved.
    private var problem: ServerList.Problem? {
        var trial = session.servers.list
        do {
            if let existing {
                try trial.edit(id: existing.id, name: name, address: address)
            } else if !isHiddenBuiltIn {
                try trial.add(name: name, address: address)
            }
            return nil
        } catch {
            return error as? ServerList.Problem
        }
    }

    /// The hidden built-in address typed back in, which brings it back.
    private var isHiddenBuiltIn: Bool {
        session.servers.list.canRestoreBuiltIn
            && ServerAddress.normalise(address) == ServerList.builtIn.address
    }

    private var title: LocalizedStringKey {
        switch target {
        case .new: "Add server"
        case .existing: "Edit server"
        }
    }

    private var typedAddress: Bool {
        !address.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    // labelled beside the field: once both are filled in,
                    // a placeholder no longer says which is which
                    HStack {
                        Text("Name").frame(width: 84, alignment: .leading)
                        TextField("Optional", text: $name)
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("server.field.name")
                    }
                    HStack {
                        Text("Address").frame(width: 84, alignment: .leading)
                        TextField("mail.example.com", text: $address)
                            .neverCapitalised()
                            .autocorrectionDisabled()
                            .urlKeyboard()
                            .accessibilityIdentifier("server.field.address")
                    }
                    // said once something is typed, not while it is empty
                    if typedAddress, let problem {
                        Text(Self.message(problem))
                            .font(.footnote)
                            .foregroundStyle(.red)
                            .accessibilityIdentifier("server.problem")
                    }
                } footer: {
                    Text("The address your mail administrator gave you. Leave the name empty to use the address.")
                }
                if let existing {
                    Section {
                        Button("Delete server", role: .destructive) {
                            session.servers.update { $0.remove(id: existing.id) }
                            session.keepToListedServer(leaving: existing)
                            dismiss()
                        }
                        .accessibilityIdentifier("server.delete")
                    }
                }
            }
            .navigationTitle(title)
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("server.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { save() }
                        .disabled(!typedAddress || problem != nil)
                        .accessibilityIdentifier("server.save")
                }
            }
            .task {
                guard !loaded, let existing else { return }
                loaded = true
                name = existing.name
                address = ServerAddress.display(existing.address)
            }
        }
    }

    private static func message(_ problem: ServerList.Problem) -> LocalizedStringKey {
        switch problem {
        case .address: "Enter a server address, like mail.example.com"
        case .duplicate: "This server is already in the list"
        }
    }

    /// A new server becomes the chosen one; an edited one stays chosen
    /// if it was.
    private func save() {
        let servers = session.servers
        do {
            if let existing {
                let wasChosen = servers.list.entry(at: session.baseURL.absoluteString)?.id == existing.id
                try servers.update { try $0.edit(id: existing.id, name: name, address: address) }
                if wasChosen, let now = servers.list.visible.first(where: { $0.id == existing.id }) {
                    session.use(now)
                }
            } else {
                var saved: SavedServer?
                try servers.update { saved = try $0.add(name: name, address: address) }
                if let saved { session.use(saved) }
            }
            dismiss()
        } catch {
            // `problem` already says what is wrong, on screen
        }
    }
}
