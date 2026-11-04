import Foundation

/// The reader's calendar, a week at a time, read only.
///
/// The rules are the web's `lib/calendar-grid.ts`: weeks start on Monday;
/// a timed event with no end lasts an hour, as the server reads it; an
/// all-day event comes as dates (`YYYY-MM-DD`, end exclusive) and covers
/// those days from local midnight to local midnight wherever the reader
/// is — never placed through UTC, which moved it a day for half the world.
/// Backend: `crates/webapi/src/handlers/calendar.rs` — `get_events` (`EventRow`).
enum CalendarWeek {
    struct Event: Decodable, Identifiable, Equatable, Sendable {
        let uid: String
        let summary: String?
        let dtstart: String?
        let dtend: String?

        var id: String { uid }
    }

    struct Span: Equatable {
        let allDay: Bool
        let start: Date
        let end: Date
    }

    /// The Monday on or before `day`, at local midnight.
    static func startOfWeek(_ day: Date, calendar: Calendar) -> Date {
        let midnight = calendar.startOfDay(for: day)
        let weekday = calendar.component(.weekday, from: midnight)
        // `weekday` is 1 for Sunday; days since Monday
        let sinceMonday = (weekday + 5) % 7
        return calendar.date(byAdding: .day, value: -sinceMonday, to: midnight) ?? midnight
    }

    /// The seven days from `monday`.
    static func days(from monday: Date, calendar: Calendar) -> [Date] {
        (0..<7).compactMap { calendar.date(byAdding: .day, value: $0, to: monday) }
    }

    /// When the event is, in the reader's zone; nil when it has no start.
    static func span(_ ev: Event, calendar: Calendar) -> Span? {
        guard let dtstart = ev.dtstart else { return nil }
        if let first = localDate(dtstart, calendar: calendar) {
            let afterLast = localDate(ev.dtend ?? "", calendar: calendar)
                ?? calendar.date(byAdding: .day, value: 1, to: first) ?? first
            return Span(allDay: true, start: first, end: afterLast)
        }
        guard let start = instant(dtstart) else { return nil }
        let end = ev.dtend.flatMap(instant) ?? start.addingTimeInterval(3600)
        return Span(allDay: false, start: start, end: end)
    }

    /// The events touching `day`: all-day ones first, then by start.
    static func events(_ events: [Event], on day: Date, calendar: Calendar) -> [Event] {
        let from = calendar.startOfDay(for: day)
        // a day is not always 24 hours where the clocks change
        guard let to = calendar.date(byAdding: .day, value: 1, to: from) else { return [] }
        let touching: [(Event, Span)] = events.compactMap { ev in
            guard let s = span(ev, calendar: calendar), s.start < to, s.end > from else { return nil }
            return (ev, s)
        }
        return touching.sorted { a, b in
            if a.1.allDay != b.1.allDay { return a.1.allDay }
            return a.1.start < b.1.start
        }.map(\.0)
    }

    /// The query the server takes: the Monday to the Monday after, as instants.
    static func window(monday: Date, calendar: Calendar) -> (start: String, end: String) {
        let next = calendar.date(byAdding: .day, value: 7, to: monday) ?? monday
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return (f.string(from: monday), f.string(from: next))
    }

    /// `2026-11-04` as that day's local midnight; nil for anything else.
    static func localDate(_ text: String, calendar: Calendar) -> Date? {
        let parts = text.split(separator: "-")
        guard text.count == 10, parts.count == 3,
              let y = Int(parts[0]), let m = Int(parts[1]), let d = Int(parts[2])
        else { return nil }
        return calendar.date(from: DateComponents(year: y, month: m, day: d))
    }

    /// The key a day's cell and its events are named by in the interface.
    static func dayKey(_ day: Date, calendar: Calendar) -> String {
        let c = calendar.dateComponents([.year, .month, .day], from: day)
        return String(format: "%04d-%02d-%02d", c.year ?? 0, c.month ?? 0, c.day ?? 0)
    }

    private static func instant(_ text: String) -> Date? {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        if let d = f.date(from: text) { return d }
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f.date(from: text)
    }
}

@MainActor
extension MailrsClient {
    func calendarEvents(monday: Date, calendar: Calendar) async throws -> [CalendarWeek.Event] {
        let w = CalendarWeek.window(monday: monday, calendar: calendar)
        var parts = URLComponents()
        parts.queryItems = [URLQueryItem(name: "start", value: w.start), URLQueryItem(name: "end", value: w.end)]
        return try await getJSON("/api/calendar/events?" + (parts.percentEncodedQuery ?? ""))
    }
}
