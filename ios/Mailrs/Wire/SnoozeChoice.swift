import Foundation

/// Putting a conversation away until later.
///
/// The same shape as `SendSchedule` and deliberately so: both answer
/// "when", both are read in the phone's own calendar, and a mail app
/// that says "tomorrow morning" about sending and something else
/// about snoozing has two vocabularies for one idea.
enum SnoozeChoice: CaseIterable, Identifiable, Sendable {
    case laterToday
    case tomorrowMorning
    case thisWeekend
    case nextWeek

    var id: Self { self }

    var label: String {
        switch self {
        case .laterToday: return String(localized: "Later today")
        case .tomorrowMorning: return String(localized: "Tomorrow morning")
        case .thisWeekend: return String(localized: "This weekend")
        case .nextWeek: return String(localized: "Next week")
        }
    }

    /// Epoch seconds, in the reader's own zone.
    ///
    /// "Morning" is 8am where the phone is, not 8am UTC: a thread that
    /// comes back in the middle of the night is worse than one that
    /// never left.
    func fireDate(after now: Date, calendar: Calendar) -> Int64? {
        guard let date = date(after: now, calendar: calendar) else { return nil }
        return Int64(date.timeIntervalSince1970)
    }

    func date(after now: Date, calendar: Calendar) -> Date? {
        switch self {
        case .laterToday:
            // Three hours on, not an evening clock time. Chosen at
            // 11pm there is no evening left to come back in, and
            // silently rolling to tomorrow is a different promise.
            return now.addingTimeInterval(3 * 60 * 60)
        case .tomorrowMorning:
            return calendar.date(byAdding: .day, value: 1, to: now)
                .flatMap { morning(of: $0, calendar: calendar) }
        case .thisWeekend:
            return next(weekday: 7, after: now, calendar: calendar)
                .flatMap { morning(of: $0, calendar: calendar) }
        case .nextWeek:
            return next(weekday: 2, after: now, calendar: calendar)
                .flatMap { morning(of: $0, calendar: calendar) }
        }
    }

    private func morning(of day: Date, calendar: Calendar) -> Date? {
        calendar.date(bySettingHour: 8, minute: 0, second: 0, of: day)
    }

    /// The next such weekday strictly after `now` — never today, so
    /// "next week" said on a Monday is the Monday coming and not one
    /// that has already begun.
    private func next(weekday: Int, after now: Date, calendar: Calendar) -> Date? {
        var day = now
        for _ in 0..<8 {
            guard let step = calendar.date(byAdding: .day, value: 1, to: day) else { return nil }
            day = step
            if calendar.component(.weekday, from: day) == weekday { return day }
        }
        return nil
    }
}
