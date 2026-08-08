import SwiftUI

/// The date on a row, in the reader's zone and language.
///
/// `RowDate`'s ladder was written for the conversation list and used
/// there alone: the thread's message rows, the sent list, the DMARC
/// report and the audit log each printed their own `.month().day()`
/// instead. So a message that arrived ten minutes ago read `20:17` in
/// the list and `8/8` in the thread it belongs to — the same fact, two
/// answers, and the less useful one on the screen you opened to read
/// it.
///
/// Three forms, because there are three kinds of fact:
///
/// - `.ladder` — a row being scanned. Time today, weekday this week,
///   date past that; see `RowDate`.
/// - `.stamp` — one row being read on purpose. Absolute, with the year.
/// - `.day` — a window rather than a moment. No clock.
///
/// The view reads the environment itself rather than taking a calendar,
/// so a caller cannot forget to graft the chosen time zone on. It also
/// carries the caption/secondary styling every one of those sites had
/// separately, so the type cannot drift either.
struct RowDateText: View {
    enum Style {
        case ladder
        case stamp
        case day
    }

    let epochSeconds: Int64
    var style: Style = .ladder

    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone
    @Environment(\.locale) private var locale

    var body: some View {
        Text(verbatim: text)
            .font(.caption)
            .foregroundStyle(.secondary)
            // A date is one line or it is nothing. Squeezed between a
            // sender name and two badges, `Aug 5, 2025 at 10:20 PM`
            // wrapped to **one character per line** and ran down the
            // side of the card — the row that cannot fit is the defect
            // this app does not ship, and a stamp has no useful
            // truncation, so it takes its width and the rest yields.
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: false)
    }

    private var text: String {
        let reader = Calendar.reader(calendar, timeZone, locale)
        switch style {
        case .ladder: return RowDate.label(epochSeconds: epochSeconds, calendar: reader)
        case .stamp: return RowDate.stamp(epochSeconds: epochSeconds, calendar: reader)
        case .day: return RowDate.day(epochSeconds: epochSeconds, calendar: reader)
        }
    }
}

extension Calendar {
    /// The calendar the reader actually reads in.
    ///
    /// The environment's calendar carries the language but not the
    /// chosen time zone — they are separate keys, and a date bucketed in
    /// the phone's zone while printed in a chosen one is a row that
    /// disagrees with itself. Assembled in one place because two views
    /// need it and a third will.
    static func reader(_ calendar: Calendar, _ timeZone: TimeZone, _ locale: Locale) -> Calendar {
        var reader = calendar
        reader.timeZone = timeZone
        reader.locale = locale
        return reader
    }
}
