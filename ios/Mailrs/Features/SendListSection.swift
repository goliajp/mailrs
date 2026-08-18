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
        if rows.isEmpty && session.scheduledSends.isEmpty {
            if session.initialLoading {
                ProgressView()
            } else {
                ContentUnavailableView(
                    "Nothing sent yet", systemImage: "paperplane",
                    description: Text("Messages you send appear here with their delivery state.")
                )
            }
        } else {
            List {
                // Above what has already gone: this is the only screen
                // that can stop a scheduled message, and it is worth
                // nothing below fifty delivered ones.
                if !session.scheduledSends.isEmpty {
                    Section("Scheduled") {
                        ForEach(session.scheduledSends) { pending in
                            ScheduledRowView(send: pending)
                        }
                    }
                }
                Section {
                    ForEach(rows) { row in
                        SendRowView(row: row)
                    }
                }
            }
            .listStyle(.plain)
            .refreshable {
                await session.loadSendRows()
                await session.loadScheduled()
            }
        }
    }
}

private struct SendRowView: View {
    @Environment(Session.self) private var session
    let row: SendJoin.Row

    @State private var redrafting: Wire.Redraft?
    /// The bytes that left, once fetched. A wrapper because
    /// `sheet(item:)` needs an identity and a `String` has none.
    @State private var source: SourceText?

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
                RowDateText(epochSeconds: row.date)
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
        // Offered only where the server says the bytes are still
        // there: against anything else it answers 409, and a button
        // that fails after the tap is worse than no button.
        .swipeActions(edge: .leading) {
            if row.canResend {
                // Edit first, and not destructive: a send that failed
                // because the address was wrong fails again unchanged,
                // which is what "Send again" does.
                Button {
                    Task { redrafting = await session.redraft(row) }
                } label: {
                    Label("Edit", systemImage: "square.and.pencil")
                }
                .tint(.accentColor)
                Button {
                    Task { await session.resend(row) }
                } label: {
                    Label("Send again", systemImage: "arrow.clockwise")
                }
                .tint(.orange)
            }
        }
        .contextMenu {
            if row.canResend {
                Button("Edit and send again", systemImage: "square.and.pencil") {
                    Task { redrafting = await session.redraft(row) }
                }
                Button("Send again", systemImage: "arrow.clockwise") {
                    Task { await session.resend(row) }
                }
            }
            // The bytes that actually left. Worth reading when a send
            // failed: they are what a resend would put back on the
            // wire.
            if row.sendId != nil {
                Button("View source", systemImage: "doc.plaintext") {
                    Task {
                        if let text = await session.sendSource(row) {
                            source = SourceText(text: text)
                        }
                    }
                }
            }
        }
        .sheet(item: $redrafting) { draft in
            ComposeView(redrafting: draft)
        }
        .sheet(item: $source) { held in
            MessageSourceSheet(text: held.text)
        }
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


/// One message that has not left yet.
private struct ScheduledRowView: View {
    @Environment(Session.self) private var session
    let send: Wire.ScheduledSend

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: "clock")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                ValueOrPlaceholder(value: send.subject, placeholder: "(no subject)")
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                Text(
                    RowDate.stamp(epochSeconds: send.scheduledAt)
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .swipeActions {
            Button(role: .destructive) {
                Task { await session.cancelScheduled(send) }
            } label: {
                Label("Cancel", systemImage: "xmark")
            }
        }
        .contextMenu {
            // Changing the time, rather than cancelling and writing it
            // again: the message is already composed, and re-typing it
            // is not what "an hour later would be better" means.
            ForEach(SendSchedule.allCases.filter { $0 != .now }) { option in
                Button(option.label) {
                    guard let when = option.fireDate(after: Date(), calendar: .current) else {
                        return
                    }
                    Task { await session.rescheduleScheduled(send, to: when) }
                }
            }
        }
    }
}

/// The source of one send, carried to a sheet that needs an identity.
private struct SourceText: Identifiable {
    let text: String
    let id = UUID()
}
