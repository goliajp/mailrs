import Foundation

/// When an invitation is, as the card says it: the reader's own time, the
/// organiser's beside it, and for a recurring meeting how it repeats.
///
/// The rule is expanded on the server, which sends the next occurrence as
/// an instant (`calendar_events::series::annotate`); this only formats.
/// Every instant goes through `TimeZone`'s database at that instant — the
/// reader's zone for the main line, the organiser's for the second — so
/// neither side's clock change is applied from a fixed offset. The same
/// rules as the web's `lib/invite-recurrence.ts` and Android's
/// `wire/InviteRepeat.kt`.
enum InviteRepeat {
    /// A rule, read for showing in words (`mailrs_ical::recur::Rule`).
    struct Rule: Decodable, Sendable, Equatable {
        let freq: String
        let interval: Int
        let byDay: [String]
        let byMonthDay: [Int]
        let count: Int?
        /// The last day, `YYYY-MM-DD`, in the organiser's zone.
        let until: String?
        let plain: Bool

        enum CodingKeys: String, CodingKey {
            case freq, interval, count, until, plain
            case byDay = "by_day"
            case byMonthDay = "by_month_day"
        }
    }

    /// One occurrence: RFC 3339 instants, or dates for an all-day series.
    struct Occurrence: Decodable, Sendable, Equatable {
        let start: String
        let end: String
        let organiserLocal: String?

        enum CodingKeys: String, CodingKey {
            case start, end
            case organiserLocal = "organiser_local"
        }
    }

    /// The time line: the next occurrence of a series, else the event itself.
    static func whenLine(_ invite: Wire.Invite, reader: TimeZone = .current, locale: Locale = .current) -> String? {
        let next = invite.next
        guard let starts = next.flatMap({ instant($0.start) }) ?? invite.startsAt else {
            // an all-day event has no instant and must not be given one: its days, as written
            if let next {
                return InviteDays.line(firstDay: next.start, dayAfterLast: next.end, locale: locale)
                    .map { words(locale).next($0) }
            }
            return InviteDays.line(firstDay: invite.firstDay, dayAfterLast: invite.dayAfterLast, locale: locale)
        }
        let style = Date.FormatStyle(date: .abbreviated, time: .shortened, locale: locale, timeZone: reader)
        var line = starts.formatted(style)
        if next != nil { line = words(locale).next(line) }
        guard let organiser = organiserClock(starts, invite, reader: reader, locale: locale) else { return line }
        return "\(line) · \(organiser)"
    }

    /// "每周四 · 到 2027年4月29日 为止"; nil for an invitation that does not repeat.
    static func ruleLine(_ invite: Wire.Invite, reader: TimeZone = .current, locale: Locale = .current) -> String? {
        guard let rule = invite.rule else { return nil }
        let w = words(locale)
        var parts = [ruleWords(rule, firstDay: invite.organiserWallClock, locale: locale)]
        if let until = rule.until, let day = InviteDays.line(firstDay: until, dayAfterLast: nil, locale: locale) {
            parts.append(w.until(day))
        }
        if let count = rule.count { parts.append(w.count(count)) }
        var line = parts.joined(separator: " · ")
        if organiserZoneDiffers(invite, reader: reader) { line += w.organiserTime }
        if invite.next == nil { line += " · " + w.ended }
        return line
    }

    /// "22:00 PDT": the instant in the organiser's IANA zone, with the
    /// abbreviation in force on that date; nil when the reader is in it.
    /// Without an IANA name, the wall-clock read off the invitation with
    /// its `TZID`, as before.
    static func organiserClock(_ at: Date, _ invite: Wire.Invite, reader: TimeZone, locale: Locale) -> String? {
        if let name = invite.organiserIana, let zone = TimeZone(identifier: name) {
            if zone.identifier == reader.identifier { return nil }
            let time = at.formatted(Date.FormatStyle(date: .omitted, time: .shortened, locale: locale, timeZone: zone))
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US")
            f.timeZone = zone
            f.dateFormat = "zzz"
            return "\(time) \(f.string(from: at))"
        }
        guard
            let tzName = invite.organiserZone,
            let wall = invite.next?.organiserLocal ?? invite.organiserWallClock,
            InviteMethod.zoneDiffers(tzName)
        else { return nil }
        return "\(String(wall.dropFirst(11).prefix(5))) \(tzName)"
    }

    private static func organiserZoneDiffers(_ invite: Wire.Invite, reader: TimeZone) -> Bool {
        if let name = invite.organiserIana { return name != reader.identifier }
        guard let tzName = invite.organiserZone else { return false }
        return InviteMethod.zoneDiffers(tzName)
    }

    private static let days = ["MO": 2, "TU": 3, "WE": 4, "TH": 5, "FR": 6, "SA": 7, "SU": 1]

    private static func ruleWords(_ rule: Rule, firstDay: String?, locale: Locale) -> String {
        let w = words(locale)
        guard rule.plain, rule.interval >= 1 else { return w.other }
        let n = rule.interval
        switch rule.freq {
        case "DAILY":
            if n == 1 { return w.daily }
            return w.dailyN(n)
        case "WEEKLY":
            return weekly(rule, firstDay: firstDay, locale: locale)
        case "MONTHLY":
            if n == 1, rule.byMonthDay.count == 1, rule.byDay.isEmpty { return w.monthlyDay(rule.byMonthDay[0]) }
            if n == 1 { return w.monthly }
            return w.other
        case "YEARLY":
            if n == 1 { return w.yearly }
            return w.other
        default:
            return w.other
        }
    }

    private static func weekly(_ rule: Rule, firstDay: String?, locale: Locale) -> String {
        let w = words(locale)
        var codes = rule.byDay
        if codes.isEmpty, let first = weekdayCode(firstDay) { codes = [first] }
        let numbers = codes.compactMap { days[$0] }
        guard !numbers.isEmpty, numbers.count == codes.count else { return w.other }
        var calendar = Calendar(identifier: .gregorian)
        calendar.locale = locale
        // Chinese says 周四 inside 每周四; the other languages use the full name
        var symbols = calendar.weekdaySymbols
        if w.shortDays { symbols = calendar.shortWeekdaySymbols }
        let names = numbers.map { symbols[$0 - 1] }.joined(separator: w.daySeparator)
        if rule.interval == 1 { return w.weekly(names) }
        return w.weeklyN(rule.interval, names)
    }

    /// The weekday code (`TH`) of a `YYYY-MM-DD…` wall-clock date.
    private static func weekdayCode(_ local: String?) -> String? {
        guard let local, local.count >= 10 else { return nil }
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let parts = local.prefix(10).split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3,
              let day = calendar.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
        else { return nil }
        let weekday = calendar.component(.weekday, from: day)
        return days.first { $0.value == weekday }?.key
    }

    private static func instant(_ text: String) -> Date? {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f.date(from: text)
    }
}
