import Foundation

/// The date a list row wears — Apple Mail's ladder.
///
/// The ladder front-loads whatever changes a decision: mail from today
/// shows the *time* (open now, or after the meeting?), this week the
/// weekday, and only past that a date. A flat "Aug 5" on today's mail
/// hides exactly the freshness that decides.
enum RowDate {
    enum Bucket: Equatable {
        case time
        case yesterday
        case weekday
        case sameYearDate
        case datedWithYear
    }

    /// Pure and injected, so the ladder is testable without freezing a
    /// locale into the output.
    static func bucket(for date: Date, now: Date, calendar: Calendar = .current) -> Bucket {
        if calendar.isDate(date, inSameDayAs: now) { return .time }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now),
           calendar.isDate(date, inSameDayAs: yesterday) {
            return .yesterday
        }
        if let weekAgo = calendar.date(byAdding: .day, value: -6, to: now),
           date >= calendar.startOfDay(for: weekAgo), date <= now {
            return .weekday
        }
        if calendar.component(.year, from: date) == calendar.component(.year, from: now) {
            return .sameYearDate
        }
        return .datedWithYear
    }

    static func label(epochSeconds: Int64, now: Date = Date(), calendar: Calendar = .current) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(epochSeconds))
        let formatter = DateFormatter()
        formatter.calendar = calendar
        switch bucket(for: date, now: now, calendar: calendar) {
        case .time:
            formatter.timeStyle = .short
            formatter.dateStyle = .none
        case .yesterday:
            formatter.doesRelativeDateFormatting = true
            formatter.dateStyle = .medium
            formatter.timeStyle = .none
        case .weekday:
            formatter.setLocalizedDateFormatFromTemplate("EEE")
        case .sameYearDate:
            formatter.setLocalizedDateFormatFromTemplate("MMMd")
        case .datedWithYear:
            formatter.dateStyle = .short
            formatter.timeStyle = .none
        }
        return formatter.string(from: date)
    }
}
