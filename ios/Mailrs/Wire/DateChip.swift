import Foundation

/// One shape for every date chip, whatever the writer typed.
///
/// A suggestion quotes the source text back, which is honest and is
/// what an inline underline would show. Out of context in a row it
/// reads as noise: `Aug 21 2026` beside `2026-08-20` beside
/// `2026-08-21` looks like three unrelated things rather than three
/// days. The written form stays as the accessibility label and as the
/// event's summary, so nothing is lost — only the row is made readable.
///
/// Built from the parts rather than by parsing `date` as an instant: a
/// bare `YYYY-MM-DD` is UTC midnight and would render as the day before
/// for any reader west of Greenwich.
enum DateChip {
    static func label(_ s: Wire.DateSuggestion) -> String {
        let parts = s.date.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3 else { return s.text }
        var c = DateComponents()
        c.year = parts[0]; c.month = parts[1]; c.day = parts[2]

        let clock = s.datetime?.split(separator: "T").dropFirst().first
        let hm = clock?.split(separator: ":").compactMap { Int($0) } ?? []
        let hasTime = hm.count >= 2
        if hasTime { c.hour = hm[0]; c.minute = hm[1] }

        guard let day = Calendar.current.date(from: c) else { return s.text }
        let f = DateFormatter()
        f.setLocalizedDateFormatFromTemplate(hasTime ? "EEEdMMMjmm" : "EEEdMMM")
        return f.string(from: day)
    }
}
