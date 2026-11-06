import Foundation

/// A team conversation's notes and overview, and the calendar: all read
/// only here, so a failure shows in the banner and leaves the screen empty
/// rather than inventing an answer.
extension Session {
    func teamNotes(_ c: TeamWire.Conv) async -> [TeamWire.Note]? {
        guard let client else { return nil }
        do {
            return try await client.teamNotes(c)
        } catch {
            banner = AppText.string("The team's notes could not be loaded")
            return nil
        }
    }

    func replyHint(_ c: TeamWire.Conv) async -> TeamWire.ReplyHint? {
        guard let client else { return nil }
        do {
            return TeamWire.replyHint(try await client.teamOverview(c), me: role?.address ?? "")
        } catch {
            banner = AppText.string("Who else is on this conversation could not be loaded")
            return nil
        }
    }

    func calendarWeek(monday: Date, calendar: Calendar) async -> [CalendarWeek.Event]? {
        guard let client else { return nil }
        do {
            return try await client.calendarEvents(monday: monday, calendar: calendar)
        } catch {
            banner = AppText.string("The calendar could not be loaded")
            return nil
        }
    }
}
