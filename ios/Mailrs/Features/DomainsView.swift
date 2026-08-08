import SwiftUI

/// Domains this server accepts mail for.
struct DomainsView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var domains: [Wire.Domain] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var adding = false
    @State private var name = ""
    @State private var pendingDelete: Wire.Domain?

    var body: some View {
        NavigationStack {
            Group {
                if loading, domains.isEmpty {
                    ProgressView()
                } else if let failure, domains.isEmpty {
                    ContentUnavailableView("Could not load domains",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(failure))
                } else if domains.isEmpty {
                    ContentUnavailableView("No domains", systemImage: "globe")
                } else {
                    List(domains) { domain in
                        HStack {
                            Image(systemName: "globe")
                                .foregroundStyle(.secondary)
                            Text(domain.name)
                                .font(.subheadline)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                        .swipeActions {
                            Button(role: .destructive) {
                                pendingDelete = domain
                            } label: {
                                Label("Delete", systemImage: "trash")
                            }
                        }
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("Domains")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        adding = true
                    } label: {
                        Label("Add domain", systemImage: "plus")
                    }
                }
            }
            .alert("Add domain", isPresented: $adding) {
                TextField("golia.jp", text: $name)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                Button("Add") { Task { await add() } }
                Button("Cancel", role: .cancel) { name = "" }
            }
            .alert("Delete domain?", isPresented: deleteBinding, presenting: pendingDelete) { domain in
                Button("Delete", role: .destructive) {
                    Task { await delete(domain) }
                    pendingDelete = nil
                }
                Button("Cancel", role: .cancel) { pendingDelete = nil }
            } message: { domain in
                // Named, and with its consequence: removing a domain
                // stops mail for every address under it.
                Text(verbatim: "\(domain.name) — mail to every address on it will stop being accepted.")
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
            domains = try await session.domains()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    private func add() async {
        let wanted = name.trimmingCharacters(in: .whitespaces).lowercased()
        name = ""
        guard wanted.contains("."), !wanted.hasSuffix(".") else { return }
        do {
            try await session.addDomain(name: wanted)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }

    private func delete(_ domain: Wire.Domain) async {
        do {
            try await session.deleteDomain(name: domain.name)
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}
