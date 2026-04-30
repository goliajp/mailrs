// InviteCard — SwiftUI mirror of web/src/components/invite-card.tsx for the
// macapp client. Renders a parsed iTIP invite (mailrs::ical::ParsedInvite
// serialized to JSON) with method-driven badge, local-tz datetime range,
// organizer/attendees, conflict pane, and RSVP / counter actions.
//
// Zero EventKit / Calendar.app interop: per MRS-1 boundary, the user's
// macOS Calendar.app subscribes to mailrs's CalDAV server externally; this
// view only talks to the mailrs HTTP API.
//
// All API calls go through `InviteService` which wraps the existing
// `ApiClient` actor (auth + base URL handled there).

import SwiftUI

// MARK: - Models

/// Mirrors the JSON shape `mailrs::ical::ParsedInvite` emits via
/// derive(Serialize). Keep it tolerant: future fields the server adds
/// land as silently-decoded extras.
struct InvitePayload: Decodable, Hashable, Sendable {
    let method: String
    let uid: String
    let sequence: Int
    let summary: String
    let location: String?
    let description: String?
    let dtstart: CalDateTime?
    let dtend: CalDateTime?
    let recurrence_id: CalDateTime?
    let organizer: Person?
    let attendees: [Attendee]?
    let rrule: String?

    struct Attendee: Decodable, Hashable, Sendable {
        let email: String
        let cn: String?
        let partstat: String
        let role: String
        let rsvp: Bool
    }

    struct Person: Decodable, Hashable, Sendable {
        let email: String
        let cn: String?
    }
}

/// `CalDateTime` decoding for the four mailrs::ical variants:
///   `{"Utc": "2026-05-01T14:00:00Z"}`
///   `{"Floating": "2026-05-01T14:00:00"}`
///   `{"Zoned": {"tz_name": "Asia/Tokyo", "local": "..."}}`
///   `{"Date": "2026-05-01"}`
struct CalDateTime: Decodable, Hashable, Sendable {
    let kind: String        // "Utc" / "Floating" / "Zoned" / "Date" / ""
    let primary: String     // ISO string for Utc / Floating / Date; "local" half for Zoned
    let tzName: String?     // only for Zoned

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: AnyCodingKey.self)
        guard let firstKey = container.allKeys.first else {
            self.kind = ""
            self.primary = ""
            self.tzName = nil
            return
        }
        self.kind = firstKey.stringValue

        if let single = try? container.decode(String.self, forKey: firstKey) {
            self.primary = single
            self.tzName = nil
            return
        }
        if let zoned = try? container.decode(ZonedInner.self, forKey: firstKey) {
            self.primary = zoned.local
            self.tzName = zoned.tz_name
            return
        }
        self.primary = ""
        self.tzName = nil
    }

    /// Render this date-time in the user's local time zone for display.
    /// Falls back to the raw string when parsing fails.
    var localDisplay: String {
        guard let date = parseUTC(primary) ?? parseNaive(primary) else { return primary }
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .short
        return f.string(from: date)
    }

    /// UTC `Date` for the RSVP recurrence_id field. Best-effort — Zoned
    /// without explicit conversion just returns the local instant treated
    /// as UTC, which is enough for round-trip with the server.
    var asUTCDate: Date? {
        parseUTC(primary) ?? parseNaive(primary)
    }

    private func parseUTC(_ s: String) -> Date? {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        if let d = f.date(from: s) { return d }
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f.date(from: s)
    }

    private func parseNaive(_ s: String) -> Date? {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(secondsFromGMT: 0)
        f.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        if let d = f.date(from: s) { return d }
        f.dateFormat = "yyyy-MM-dd"
        return f.date(from: s)
    }

    private struct ZonedInner: Decodable {
        let tz_name: String
        let local: String
    }
}

/// Used by the keyed-container approach for externally-tagged enums.
private struct AnyCodingKey: CodingKey {
    var stringValue: String
    var intValue: Int?
    init(stringValue: String) {
        self.stringValue = stringValue
        self.intValue = nil
    }
    init?(intValue: Int) { nil }
}

/// `GET /api/mail/messages/{uid}` envelope — only the invite fields are
/// modelled here. The macapp's `ThreadMessage` is the rich variant for
/// list rendering; this thin one is for the InviteCard's lazy fetch.
struct InviteMessageDetail: Decodable, Sendable {
    let uid: Int
    let invite_payload: InvitePayload?
    let invite_method: String?
}

struct InviteConflictRow: Decodable, Hashable, Sendable, Identifiable {
    let uid: String
    let summary: String
    let dtstart: Date?
    let dtend: Date?
    let organizer: String?
    let status: String?

    var id: String { uid }
}

struct InviteRsvpResult: Decodable, Sendable {
    let success: Bool
    let message: String?
}

// MARK: - Service

struct InviteService {
    let api: ApiClient

    func fetchDetail(messageUid: Int) async throws -> InviteMessageDetail {
        try await api.get("/api/mail/messages/\(messageUid)")
    }

    func fetchConflicts(start: Date, end: Date, excludeUid: String?) async throws -> [InviteConflictRow] {
        let isoStart = ISO8601DateFormatter().string(from: start)
        let isoEnd = ISO8601DateFormatter().string(from: end)
        var query = [
            URLQueryItem(name: "start", value: isoStart),
            URLQueryItem(name: "end", value: isoEnd),
        ]
        if let excludeUid {
            query.append(URLQueryItem(name: "exclude_uid", value: excludeUid))
        }
        return try await api.get("/api/calendar/conflicts", query: query)
    }

    func sendRsvp(messageUid: Int, partstat: String, recurrenceId: Date?) async throws -> InviteRsvpResult {
        struct Body: Encodable {
            let partstat: String
            let recurrence_id: String?
        }
        let rid = recurrenceId.map { ISO8601DateFormatter().string(from: $0) }
        return try await api.post(
            "/api/invites/\(messageUid)/rsvp",
            body: Body(partstat: partstat, recurrence_id: rid)
        )
    }

    func sendCounter(messageUid: Int, dtstart: Date, dtend: Date?) async throws -> InviteRsvpResult {
        struct Body: Encodable {
            let dtstart: String
            let dtend: String?
        }
        return try await api.post(
            "/api/invites/\(messageUid)/counter",
            body: Body(
                dtstart: ISO8601DateFormatter().string(from: dtstart),
                dtend: dtend.map { ISO8601DateFormatter().string(from: $0) }
            )
        )
    }
}

// MARK: - View

@MainActor
struct InviteCardView: View {
    let messageUid: Int
    let inviteService: InviteService

    @State private var detail: InviteMessageDetail?
    @State private var conflicts: [InviteConflictRow] = []
    @State private var rsvpState: RsvpState = .idle
    @State private var counterOpen = false
    @State private var counterStart: Date = .now
    @State private var counterEnd: Date = Date(timeIntervalSinceNow: 3600)

    enum RsvpState: Equatable {
        case idle
        case pending
        case sent
        case error(String)
    }

    var body: some View {
        Group {
            if let detail, let payload = detail.invite_payload {
                cardContent(payload: payload)
            } else {
                EmptyView()
            }
        }
        .task(id: messageUid) {
            await load()
        }
    }

    @ViewBuilder
    private func cardContent(payload: InvitePayload) -> some View {
        let cancelled = payload.method.uppercased() == "CANCEL"
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Image(systemName: "calendar")
                    .foregroundStyle(.secondary)
                Text(badgeLabel(for: payload.method))
                    .font(.caption.bold())
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(badgeBackground(for: payload.method))
                    .foregroundStyle(badgeForeground(for: payload.method))
                    .clipShape(Capsule())
                Spacer()
            }

            Text(payload.summary).font(.headline)

            if payload.recurrence_id != nil {
                Text("ⓘ This occurrence of a recurring event — RSVP applies only to this instance.")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }

            if let range = formatRange(start: payload.dtstart, end: payload.dtend) {
                HStack(spacing: 4) {
                    Image(systemName: "clock").foregroundStyle(.secondary)
                    Text(range).font(.caption)
                }
            }

            if let location = payload.location, !location.isEmpty {
                HStack(spacing: 4) {
                    Image(systemName: "mappin.and.ellipse").foregroundStyle(.secondary)
                    Text(location).font(.caption)
                }
            }

            if let organizer = payload.organizer {
                Text("Organizer: \(organizerLabel(organizer))")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            if let attendees = payload.attendees, !attendees.isEmpty {
                HStack(spacing: 4) {
                    Image(systemName: "person.2").foregroundStyle(.secondary)
                    Text("\(attendees.count) attendee\(attendees.count == 1 ? "" : "s")")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }

            if !conflicts.isEmpty && !cancelled {
                conflictPane
            }

            if !cancelled {
                actionRow(payload: payload)
                if counterOpen {
                    counterForm()
                }
            }
        }
        .padding(12)
        .background(Color.secondary.opacity(0.05))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .task(id: detail?.uid) {
            await loadConflicts(payload: payload)
        }
    }

    private var conflictPane: some View {
        VStack(alignment: .leading, spacing: 4) {
            if conflicts.count == 1, let first = conflicts.first {
                Text("⚠ Conflicts with \(first.summary)")
                    .font(.caption)
                    .foregroundStyle(.orange)
            } else {
                Text("⚠ Conflicts with \(conflicts.count) events")
                    .font(.caption)
                    .foregroundStyle(.orange)
                ForEach(conflicts) { c in
                    Text("• \(c.summary)").font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        .padding(8)
        .background(Color.orange.opacity(0.05))
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    private func actionRow(payload: InvitePayload) -> some View {
        HStack(spacing: 8) {
            Button {
                Task { await sendRsvp("ACCEPTED", payload: payload) }
            } label: {
                Label("Accept", systemImage: "checkmark")
            }
            Button {
                Task { await sendRsvp("TENTATIVE", payload: payload) }
            } label: {
                Text("Tentative")
            }
            Button {
                Task { await sendRsvp("DECLINED", payload: payload) }
            } label: {
                Label("Decline", systemImage: "xmark")
            }
            Button {
                counterOpen.toggle()
            } label: {
                Text(counterOpen ? "Cancel counter" : "Propose new time")
            }
            statusLabel
        }
        .buttonStyle(.bordered)
        .disabled(rsvpState == .pending || rsvpState == .sent)
        .controlSize(.small)
    }

    @ViewBuilder
    private var statusLabel: some View {
        switch rsvpState {
        case .idle: EmptyView()
        case .pending: Text("sending…").font(.caption2).foregroundStyle(.secondary)
        case .sent: Text("✓ reply sent").font(.caption2).foregroundStyle(.green)
        case .error(let msg): Text("error: \(msg)").font(.caption2).foregroundStyle(.red)
        }
    }

    private func counterForm() -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Counter-proposal — your local time. Sends METHOD=COUNTER to the organizer.")
                .font(.caption2)
                .foregroundStyle(.secondary)
            DatePicker("Start", selection: $counterStart)
            DatePicker("End", selection: $counterEnd)
            Button("Send counter") {
                Task { await sendCounter() }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.small)
        }
        .padding(8)
        .background(Color.secondary.opacity(0.04))
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    // MARK: - Actions

    private func load() async {
        do {
            detail = try await inviteService.fetchDetail(messageUid: messageUid)
        } catch {
            // Silent failure: invite card simply doesn't render.
        }
    }

    private func loadConflicts(payload: InvitePayload) async {
        guard
            let dtstart = payload.dtstart,
            let startDate = dtstart.asUTCDate
        else { return }
        let endDate = payload.dtend?.asUTCDate ?? startDate
        do {
            conflicts = try await inviteService.fetchConflicts(
                start: startDate,
                end: endDate,
                excludeUid: payload.uid
            )
        } catch {
            conflicts = []
        }
    }

    private func sendRsvp(_ partstat: String, payload: InvitePayload) async {
        rsvpState = .pending
        do {
            let recurrenceId = payload.recurrence_id?.asUTCDate
            _ = try await inviteService.sendRsvp(
                messageUid: messageUid,
                partstat: partstat,
                recurrenceId: recurrenceId
            )
            rsvpState = .sent
        } catch {
            rsvpState = .error(error.localizedDescription)
        }
    }

    private func sendCounter() async {
        rsvpState = .pending
        do {
            _ = try await inviteService.sendCounter(
                messageUid: messageUid,
                dtstart: counterStart,
                dtend: counterEnd
            )
            rsvpState = .sent
            counterOpen = false
        } catch {
            rsvpState = .error(error.localizedDescription)
        }
    }

    // MARK: - Format helpers

    private func badgeLabel(for method: String) -> String {
        switch method.uppercased() {
        case "REQUEST": return "New invite"
        case "REPLY": return "Reply"
        case "CANCEL": return "Cancelled"
        case "UPDATE": return "Updated"
        case "COUNTER": return "Counter-proposed"
        case "DECLINECOUNTER": return "Counter declined"
        default: return method
        }
    }

    private func badgeBackground(for method: String) -> Color {
        switch method.uppercased() {
        case "CANCEL": return .red.opacity(0.18)
        case "COUNTER", "DECLINECOUNTER": return .orange.opacity(0.18)
        case "REPLY": return .blue.opacity(0.18)
        case "UPDATE": return .cyan.opacity(0.18)
        default: return .green.opacity(0.18)
        }
    }

    private func badgeForeground(for method: String) -> Color {
        switch method.uppercased() {
        case "CANCEL": return .red
        case "COUNTER", "DECLINECOUNTER": return .orange
        case "REPLY": return .blue
        case "UPDATE": return .cyan
        default: return .green
        }
    }

    private func organizerLabel(_ p: InvitePayload.Person) -> String {
        if let cn = p.cn, !cn.isEmpty {
            return "\(cn) <\(p.email)>"
        }
        return p.email
    }

    private func formatRange(start: CalDateTime?, end: CalDateTime?) -> String? {
        guard let start else { return nil }
        let s = start.localDisplay
        guard let end else { return s }
        return "\(s) → \(end.localDisplay)"
    }
}

// MARK: - AttachmentInfo helper

extension AttachmentInfo {
    var isCalendarPart: Bool {
        content_type.lowercased().hasPrefix("text/calendar")
    }
}
