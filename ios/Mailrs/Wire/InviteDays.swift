import Foundation

/// The days of an all-day event, in a line.
///
/// An all-day event has no instant, only dates, and a date has no offset:
/// read as midnight somewhere and moved into the reader's zone, it lands
/// on the day before for everyone west of that somewhere. So the dates
/// are shown as written, in the reader's language and without a time.
/// The server sends the first day and the day after the last, as DTEND
/// is written (`2026-11-03` and `2026-11-05` are two days, the 3rd and
/// the 4th).
enum InviteDays {
    static func line(firstDay: String?, dayAfterLast: String?, locale: Locale = .current) -> String? {
        guard let first = date(firstDay) else { return nil }
        let style = Date.FormatStyle(date: .abbreviated, time: .omitted, locale: locale, timeZone: utc)
        let last = date(dayAfterLast).flatMap { utcCalendar.date(byAdding: .day, value: -1, to: $0) }
        guard let last, last > first else { return first.formatted(style) }
        return "\(first.formatted(style)) – \(last.formatted(style))"
    }

    // Both read and written in UTC, where midnight is the day itself, so
    // no zone ever moves the date.
    private static let utc = TimeZone(identifier: "UTC")!

    private static var utcCalendar: Calendar {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = utc
        return c
    }

    private static func date(_ ymd: String?) -> Date? {
        guard let ymd else { return nil }
        let parts = ymd.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3 else { return nil }
        return utcCalendar.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
    }
}
