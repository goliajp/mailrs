import Foundation

/// Reading a `Date:` header — RFC 5322 §3.3.
///
/// A list row shows when mail arrived, and sorts on it. Getting this
/// wrong is quiet: the row shows a plausible time that is hours out,
/// or the list orders itself by nothing at all.
enum MailDate {
    /// Seconds since the epoch, or nil.
    ///
    /// Nil rather than "now": a message with an unreadable date shown
    /// as having just arrived jumps to the top of the list and stays
    /// there, which is worse than showing no date.
    static func epochSeconds(_ header: String) -> Int64? {
        // Drop a trailing comment: `+0900 (JST)` is legal and common.
        //
        // `split` on an empty string returns an **empty array**, so
        // taking `[0]` of it is a crash rather than an empty result —
        // and an empty `Date:` header is a real thing to be handed.
        let trimmed = String(header.prefix(while: { $0 != "(" }))
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return nil }

        // The day name is optional, and when present it is followed by
        // a comma. Servers disagree about the space after it.
        let body = trimmed.contains(",")
            ? String(trimmed[trimmed.index(after: trimmed.firstIndex(of: ",")!)...])
            : trimmed
        let parts = body.split(whereSeparator: \.isWhitespace).map(String.init)
        guard parts.count >= 5,
              let day = Int(parts[0]),
              let month = months.firstIndex(of: parts[1].lowercased().prefix(3).description)
                  .map({ $0 + 1 }),
              let year = year(parts[2])
        else { return nil }

        let time = parts[3].split(separator: ":").map(String.init)
        guard time.count >= 2, let hour = Int(time[0]), let minute = Int(time[1]) else {
            return nil
        }
        let second = time.count > 2 ? Int(time[2]) ?? 0 : 0
        guard let offset = offsetSeconds(parts[4]) else { return nil }

        let days = daysFromCivil(year: year, month: month, day: day)
        return days * 86_400 + Int64(hour) * 3600 + Int64(minute) * 60 + Int64(second) - offset
    }

    /// A two-digit year, as RFC 5322 §4.3 says to read one.
    ///
    /// Obsolete and still in the wild. 50–99 is 19xx, 00–49 is 20xx;
    /// reading `26` as year 26 puts the message two thousand years in
    /// the past and sorts the whole list around it.
    private static func year(_ s: String) -> Int? {
        guard let n = Int(s) else { return nil }
        switch s.count {
        case 4: return n
        case 2: return n >= 50 ? 1900 + n : 2000 + n
        case 3: return 1900 + n  // also obsolete, also seen
        default: return nil
        }
    }

    /// `+0900`, or one of the obsolete names.
    ///
    /// An unknown name is **not** zero: guessing UTC for something
    /// unknown is a silent thirteen-hour error.
    private static func offsetSeconds(_ s: String) -> Int64? {
        if s.count == 5, s.first == "+" || s.first == "-" {
            let digits = s.dropFirst()
            guard let h = Int(digits.prefix(2)), let m = Int(digits.suffix(2)) else { return nil }
            let magnitude = Int64(h) * 3600 + Int64(m) * 60
            return s.first == "-" ? -magnitude : magnitude
        }
        switch s.uppercased() {
        case "UT", "GMT", "Z": return 0
        case "EST": return -5 * 3600
        case "EDT": return -4 * 3600
        case "CST": return -6 * 3600
        case "CDT": return -5 * 3600
        case "MST": return -7 * 3600
        case "MDT": return -6 * 3600
        case "PST": return -8 * 3600
        case "PDT": return -7 * 3600
        default: return nil
        }
    }

    private static let months = [
        "jan", "feb", "mar", "apr", "may", "jun",
        "jul", "aug", "sep", "oct", "nov", "dec",
    ]

    /// Days from 1970-01-01 — Howard Hinnant's civil-from-days, which
    /// is exact and has no library behind it to disagree with.
    private static func daysFromCivil(year y0: Int, month m: Int, day d: Int) -> Int64 {
        let y = m <= 2 ? y0 - 1 : y0
        let era = (y >= 0 ? y : y - 399) / 400
        let yoe = y - era * 400
        let doy = (153 * (m > 2 ? m - 3 : m + 9) + 2) / 5 + d - 1
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
        return Int64(era) * 146_097 + Int64(doe) - 719_468
    }
}
