import SwiftUI

/// The Send list's body — drawn inside the same screen chrome as the
/// thread lists, so the picker, search field and title behave
/// identically.
struct SendListSection: View {
    /// The shared search text; matched locally. The server's staged
    /// search covers conversations, not the sends projection, so these
    /// rows answer the query the way the web's Send view does — by
    /// substring on what is visible.
    let searchText: String
    @Environment(Session.self) private var session

    private var rows: [SendJoin.Row] {
        guard let query = SearchRule.query(from: searchText) else { return session.sendRows }
        let needle = query.lowercased()
        return session.sendRows.filter {
            $0.subject.lowercased().contains(needle) || $0.to.lowercased().contains(needle)
        }
    }

    var body: some View {
        if rows.isEmpty {
            ContentUnavailableView("Nothing sent yet", systemImage: "paperplane")
        } else {
            List(rows) { row in
                SendRowView(row: row)
            }
            .listStyle(.plain)
            .refreshable { await session.loadSendRows() }
        }
    }
}

private struct SendRowView: View {
    let row: SendJoin.Row

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(row.to.isEmpty ? "(no recipient)" : row.to)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                Spacer()
                Text(Date(timeIntervalSince1970: TimeInterval(row.date)),
                     format: .dateTime.month().day())
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            HStack(spacing: 6) {
                Text(row.subject.isEmpty ? "(no subject)" : row.subject)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                StatusBadge(status: row.status)
            }
        }
        .padding(.vertical, 2)
    }
}

/// Delivery status, where there is one.
///
/// Absence renders nothing: most mail predates the projection, and a
/// default badge would claim knowledge the row does not have. Only the
/// states needing attention are loud; `delivered` is quiet text because
/// it is the ordinary case.
private struct StatusBadge: View {
    let status: String?

    var body: some View {
        switch status {
        case "failed":
            Label("Failed", systemImage: "exclamationmark.circle.fill")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.red)
        case "partial":
            Label("Partial", systemImage: "exclamationmark.triangle.fill")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.orange)
        case "sending", "scheduled":
            Text(status ?? "")
                .font(.caption2)
                .foregroundStyle(.secondary)
        case "delivered":
            Image(systemName: "checkmark")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .accessibilityLabel("Delivered")
        default:
            EmptyView()
        }
    }
}
