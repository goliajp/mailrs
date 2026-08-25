import SwiftUI

/// The senders this account always allows, and always blocks.
///
/// `spam:{user}:whitelist` is live and consequential: marking a
/// conversation *not junk* adds its sender, and the inbound pipeline
/// reads the set on every delivery. The routes that show and edit it
/// have existed since before this client did, with no caller on any
/// platform — so the list could only grow, and a sender added by one
/// mistaken tap kept bypassing the filter with nothing able to show
/// that it was there.
struct SenderListsView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss
    @State private var entries: [SenderListKind: [String]] = [:]
    @State private var loading = true
    @State private var failure: String?
    @State private var adding: SenderListKind?
    @State private var newAddress = ""

    var body: some View {
        NavigationStack {
            List {
                if let failure {
                    Section {
                        Text(failure).foregroundStyle(.red)
                    }
                }
                ForEach(SenderListKind.allCases) { kind in
                    Section {
                        let rows = entries[kind] ?? []
                        if loading {
                            ProgressView()
                        } else if rows.isEmpty {
                            Text("No addresses").foregroundStyle(.secondary)
                        }
                        ForEach(rows, id: \.self) { address in
                            Text(address)
                                .font(.callout)
                                .swipeActions {
                                    Button(role: .destructive) {
                                        Task { await remove(kind, address) }
                                    } label: {
                                        Label("Remove", systemImage: "trash")
                                    }
                                }
                        }
                        Button {
                            newAddress = ""
                            adding = kind
                        } label: {
                            Label("Add address", systemImage: "plus")
                        }
                        .accessibilityIdentifier("add-\(kind.rawValue)")
                    } header: {
                        Text(kind.title)
                    } footer: {
                        Text(kind.explanation)
                    }
                }
            }
            .navigationTitle("Senders")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            // An alert, not a `confirmationDialog`: on a phone that one
            // renders as a popover and drops what it is given — twice
            // in this app already, for delete and for the bucket move.
            .alert("Add address", isPresented: Binding(
                get: { adding != nil },
                set: { if !$0 { adding = nil } }
            )) {
                TextField("someone@example.com", text: $newAddress)
                    .neverCapitalised()
                    .autocorrectionDisabled()
                Button("Add") {
                    if let kind = adding { Task { await add(kind, newAddress) } }
                }
                Button("Cancel", role: .cancel) { adding = nil }
            }
            .task { await load() }
        }
    }

    private func load() async {
        guard let client = session.client else { return }
        loading = true
        defer { loading = false }
        for kind in SenderListKind.allCases {
            do {
                entries[kind] = try await client.senderList(kind).sorted()
                failure = nil
            } catch {
                failure = error.localizedDescription
            }
        }
    }

    private func add(_ kind: SenderListKind, _ address: String) async {
        let trimmed = address.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let client = session.client else { return }
        do {
            try await client.addToSenderList(kind, address: trimmed)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }

    private func remove(_ kind: SenderListKind, _ address: String) async {
        guard let client = session.client else { return }
        // Off the screen first, then the wire — and back if the wire
        // refuses. A row that lingers reads as a delete that failed
        // silently.
        let previous = entries[kind] ?? []
        entries[kind] = previous.filter { $0 != address }
        do {
            try await client.removeFromSenderList(kind, address: address)
        } catch {
            entries[kind] = previous
            failure = error.localizedDescription
        }
    }
}
