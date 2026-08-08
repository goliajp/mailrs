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
        let formatter = formatter(calendar)
        let date = Date(timeIntervalSince1970: TimeInterval(epochSeconds))
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

    /// An exact moment, spelled out — for an opened message, an audit
    /// entry, a retry time.
    ///
    /// The ladder exists for scanning: it drops whatever the reader can
    /// infer from position in the list. Somewhere someone is reading one
    /// row on purpose, that inference is gone and the year has to be on
    /// the page. "Aug 8, 20:17" is the same string for this year and for
    /// four years ago.
    static func stamp(epochSeconds: Int64, calendar: Calendar = .current) -> String {
        let formatter = formatter(calendar)
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(epochSeconds)))
    }

    /// A whole day, with no time on it — for a window rather than a
    /// moment.
    ///
    /// A DMARC report covers a reporting period, so the ladder's clock
    /// would print a precision the fact does not have: "20:17" for
    /// something that happened all day. The year appears only when it is
    /// not this one, which is the one place the ladder's economy still
    /// applies.
    static func day(epochSeconds: Int64, now: Date = Date(), calendar: Calendar = .current) -> String {
        let formatter = formatter(calendar)
        let date = Date(timeIntervalSince1970: TimeInterval(epochSeconds))
        switch bucket(for: date, now: now, calendar: calendar) {
        case .datedWithYear:
            formatter.dateStyle = .medium
            formatter.timeStyle = .none
        default:
            formatter.setLocalizedDateFormatFromTemplate("MMMd")
        }
        return formatter.string(from: date)
    }

    private static func formatter(_ calendar: Calendar) -> DateFormatter {
        let formatter = DateFormatter()
        formatter.calendar = calendar
        // The calendar carries the reader's zone and language, and the
        // formatter has to be told both — left alone it renders in the
        // phone's, so a chosen time zone would change which bucket a
        // message fell into without changing the time printed on it.
        formatter.timeZone = calendar.timeZone
        formatter.locale = calendar.locale ?? .autoupdatingCurrent
        return formatter
    }
}
