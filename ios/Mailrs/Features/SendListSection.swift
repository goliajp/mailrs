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
            if session.initialLoading {
                ProgressView()
            } else {
                ContentUnavailableView("Nothing sent yet", systemImage: "paperplane")
            }
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

    /// The first recipient wears the avatar — a sent row's face is who
    /// it went to, mirroring the inbox where the face is who it came
    /// from.
    private var face: String {
        row.to.split(separator: ",").first.map {
            $0.trimmingCharacters(in: .whitespaces)
        } ?? ""
    }

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            // The web's `flagged` left border: a send that failed is
            // worth more attention than anything else a row can say,
            // and a status word alone is easy to scan past.
            Rectangle()
                .fill(edgeColor)
                .frame(width: 3)
                .clipShape(Capsule())
            if !face.isEmpty {
                SenderAvatar(sender: face)
            }
            VStack(alignment: .leading, spacing: 2) {
            HStack {
                ValueOrPlaceholder(value: displayedTo, placeholder: "(no recipient)")
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                Spacer()
                Text(Date(timeIntervalSince1970: TimeInterval(row.date)),
                     format: .dateTime.month().day())
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            HStack(spacing: 6) {
                ValueOrPlaceholder(value: row.subject, placeholder: "(no subject)")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                StatusBadge(status: row.status)
            }
            }
        }
        .padding(.vertical, 2)
    }

    private var edgeColor: Color {
        if row.status == "failed" { return Color.red.opacity(0.6) }
        return .clear
    }

    /// Names, not addr-specs — the same rendering the inbox rows use.
    private var displayedTo: String {
        row.to.split(separator: ",")
            .map { SenderName.extractName($0.trimmingCharacters(in: .whitespaces)) }
            .joined(separator: ", ")
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
            // `.titleAndIcon` explicitly: inside a List, the default
            // label style is the Form one, which aligns icons in their
            // own column and stretched the glyph away from its word.
            Label("Failed", systemImage: "exclamationmark.circle.fill")
                .labelStyle(.titleAndIcon)
                .chip(.red)
        case "partial":
            Label("Partial", systemImage: "exclamationmark.triangle.fill")
                .labelStyle(.titleAndIcon)
                .chip(.orange)
        case "sending", "scheduled":
            Text((status ?? "").capitalized)
                .chip(.secondary)
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

/// A status pill: the web's badge shape (rounded, tinted background,
/// the colour carried by the text rather than a solid fill), so a
/// delivery state reads as a state and not as body copy.
private extension View {
    func chip(_ tint: Color) -> some View {
        self
            .font(.caption2.weight(.semibold))
            .foregroundStyle(tint)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(tint.opacity(0.12), in: Capsule())
    }
}
