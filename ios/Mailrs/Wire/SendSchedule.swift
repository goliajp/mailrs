import Foundation

/// When a message should leave.
///
/// The server has taken `scheduled_at` since scheduling existed, the
/// sender sweeps a score-ordered zset and promotes what is due, and the
/// queue screen already draws "Scheduled for …". The phone had no way
/// to ask for it — which is the device most likely to be writing mail
/// at an hour nobody wants it delivered.
enum SendSchedule: CaseIterable, Identifiable, Sendable {
    case now
    case laterToday
    case tomorrowMorning
    case mondayMorning

    var id: Self { self }

    var label: String {
        switch self {
        case .now: return String(localized: "Send now")
        case .laterToday: return String(localized: "Later today")
        case .tomorrowMorning: return String(localized: "Tomorrow morning")
        case .mondayMorning: return String(localized: "Monday morning")
        }
    }

    /// Epoch **seconds**, or `nil` for now.
    ///
    /// Seconds and an integer, both deliberate: the handler reads
    /// anything it cannot parse as "not scheduling", which is how the
    /// web's ISO 8601 string made every scheduled send go out at once.
    /// It is a 400 today, and this side never has the chance to produce
    /// one.
    func fireDate(after now: Date, calendar: Calendar) -> Int64? {
        guard let date = date(after: now, calendar: calendar) else { return nil }
        return Int64(date.timeIntervalSince1970)
    }

    /// The moment itself, in the reader's own calendar and time zone.
    ///
    /// "Morning" is 8am where the phone is, not 8am UTC — a schedule
    /// named after a time of day and delivered in the middle of the
    /// night is worse than no scheduling at all.
    func date(after now: Date, calendar: Calendar) -> Date? {
        switch self {
        case .now:
            return nil
        case .laterToday:
            // Three hours on, not a clock time: "later today" chosen at
            // 11pm has no evening left to land in, and rolling it to
            // tomorrow would be a different choice than the one made.
            return now.addingTimeInterval(3 * 60 * 60)
        case .tomorrowMorning:
            return calendar.date(byAdding: .day, value: 1, to: now).flatMap {
                morning(of: $0, calendar: calendar)
            }
        case .mondayMorning:
            return nextMonday(after: now, calendar: calendar).flatMap {
                morning(of: $0, calendar: calendar)
            }
        }
    }

    private func morning(of day: Date, calendar: Calendar) -> Date? {
        calendar.date(bySettingHour: 8, minute: 0, second: 0, of: day)
    }

    /// The next Monday strictly after `now` — never today, even at one
    /// minute past midnight on a Monday, because "Monday morning" said
    /// on a Monday means the one coming.
    private func nextMonday(after now: Date, calendar: Calendar) -> Date? {
        var day = now
        for _ in 0..<8 {
            guard let next = calendar.date(byAdding: .day, value: 1, to: day) else { return nil }
            day = next
            if calendar.component(.weekday, from: day) == 2 { return day }
        }
        return nil
    }
}
