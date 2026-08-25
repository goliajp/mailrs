import SwiftUI

/// What was done, by whom, to what.
///
/// Every administration screen in this app writes one of these on the
/// server; this is the half that makes them answerable afterwards
/// rather than merely done. Backend:
/// `crates/webapi/src/handlers/admin_audit.rs`.
struct AuditLogView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var rows: [Wire.AuditRow] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var filter = ""

    /// The families present in what was loaded, rather than a hardcoded
    /// list: the server grows verbs without telling the client, and a
    /// fixed menu would quietly stop offering the newest ones.
    private var families: [String] {
        Array(Set(rows.map { AuditAction.family(of: $0.action) })).sorted()
    }

    var body: some View {
        NavigationStack {
            Group {
                if loading, rows.isEmpty {
                    ProgressView()
                } else if let failure, rows.isEmpty {
                    ContentUnavailableView("Could not load the audit log",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(failure))
                } else if rows.isEmpty {
                    ContentUnavailableView(
                        "Nothing recorded", systemImage: "list.bullet.rectangle",
                        description: Text("Administrative changes appear here as they happen.")
                    )
                } else {
                    List(rows) { row in
                        AuditRowView(row: row)
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("Audit log")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .leadingAction) {
                    Menu {
                        Picker("Filter", selection: $filter) {
                            Text("All").tag("")
                            ForEach(families, id: \.self) { family in
                                Text(verbatim: family).tag(family)
                            }
                        }
                    } label: {
                        Label("Filter", systemImage: "line.3.horizontal.decrease.circle")
                    }
                }
            }
            // The server filters, not the client: it scans a wider
            // window when asked for a family, so filtering here would
            // return fewer rows than asking for them does.
            .onChange(of: filter) { _, _ in Task { await load() } }
            .task { await load() }
        }
    }

    private func load() async {
        loading = true
        do {
            rows = try await session.auditLog(actionPrefix: filter)
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }
}

private struct AuditRowView: View {
    let row: Wire.AuditRow
    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone
    @Environment(\.locale) private var locale

    private var destructive: Bool {
        AuditAction.isDestructive(row.action)
    }

    private var verbColor: Color {
        if destructive { return .red }
        return .secondary
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(verbatim: AuditAction.family(of: row.action))
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                Text(verbatim: AuditAction.verb(of: row.action))
                    .font(.caption2.weight(.semibold))
                    .lineLimit(1)
                    .foregroundStyle(verbColor)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(verbColor.opacity(0.12), in: Capsule())
                Spacer(minLength: 4)
                RowDateText(epochSeconds: row.timestamp, style: .stamp)
            }
            Text(verbatim: row.target)
                .font(.subheadline)
                .lineLimit(1)
            HStack(spacing: 4) {
                SenderAvatar(sender: row.actor, size: 16)
                Text(verbatim: SenderName.extractName(row.actor))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                if !row.detail.isEmpty {
                    Text(verbatim: "· \(row.detail)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 2)
    }
}
