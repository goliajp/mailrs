import Foundation
import Testing

@testable import Mailrs

/// The same cases as Android's `CalendarWeekTest`.
@Suite("Calendar week")
struct CalendarWeekTests {
    static func calendar(_ zone: String) -> Calendar {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = TimeZone(identifier: zone)!
        return c
    }

    static func day(_ text: String, _ cal: Calendar) -> Date {
        CalendarWeek.localDate(text, calendar: cal)!
    }

    let offsite = CalendarWeek.Event(uid: "ev-offsite", summary: "Team offsite", dtstart: "2026-11-04", dtend: "2026-11-06")

    @Test func weeksStartOnMonday() {
        let cal = Self.calendar("Asia/Tokyo")
        #expect(CalendarWeek.startOfWeek(Self.day("2026-11-08", cal), calendar: cal) == Self.day("2026-11-02", cal))
        #expect(CalendarWeek.startOfWeek(Self.day("2026-11-02", cal), calendar: cal) == Self.day("2026-11-02", cal))
    }

    /// Dates, not instants: the same two days in every zone, the end day excluded.
    @Test(arguments: ["Asia/Tokyo", "America/Los_Angeles", "Pacific/Kiritimati", "Pacific/Pago_Pago"])
    func anAllDayEventCoversItsDatesWhereverTheReaderIs(_ zone: String) {
        let cal = Self.calendar(zone)
        let week = CalendarWeek.days(from: Self.day("2026-11-02", cal), calendar: cal)
        let on = week.filter { !CalendarWeek.events([offsite], on: $0, calendar: cal).isEmpty }
            .map { CalendarWeek.dayKey($0, calendar: cal) }
        #expect(on == ["2026-11-04", "2026-11-05"])
    }

    @Test func anAllDayEventWithoutAnEndIsOneDay() throws {
        let cal = Self.calendar("Asia/Tokyo")
        let span = try #require(CalendarWeek.span(
            CalendarWeek.Event(uid: "a", summary: nil, dtstart: "2026-11-04", dtend: nil), calendar: cal))
        #expect(span.allDay)
        #expect(span.end.timeIntervalSince(span.start) == 86_400)
    }

    @Test func aTimedEventWithoutAnEndLastsAnHour() throws {
        let span = try #require(CalendarWeek.span(
            CalendarWeek.Event(uid: "b", summary: nil, dtstart: "2026-11-04T00:00:00Z", dtend: nil),
            calendar: Self.calendar("UTC")))
        #expect(span.end.timeIntervalSince(span.start) == 3600)
        #expect(!span.allDay)
    }

    /// 00:00 UTC on Wednesday is Tuesday afternoon in Los Angeles.
    @Test func aTimedEventIsPlacedInTheReadersZone() {
        let call = CalendarWeek.Event(uid: "b", summary: nil, dtstart: "2026-11-04T00:00:00Z", dtend: "2026-11-04T00:30:00Z")
        let tokyo = Self.calendar("Asia/Tokyo")
        let la = Self.calendar("America/Los_Angeles")
        #expect(CalendarWeek.events([call], on: Self.day("2026-11-04", tokyo), calendar: tokyo).count == 1)
        #expect(CalendarWeek.events([call], on: Self.day("2026-11-03", la), calendar: la).count == 1)
    }

    @Test func allDayEventsComeFirst() {
        let cal = Self.calendar("Asia/Tokyo")
        let timed = CalendarWeek.Event(uid: "t", summary: nil, dtstart: "2026-11-04T00:00:00Z", dtend: nil)
        let order = CalendarWeek.events([timed, offsite], on: Self.day("2026-11-04", cal), calendar: cal).map(\.uid)
        #expect(order == ["ev-offsite", "t"])
    }

    @Test func anEventWithNoStartIsNotPlaced() {
        let cal = Self.calendar("UTC")
        #expect(CalendarWeek.span(CalendarWeek.Event(uid: "x", summary: nil, dtstart: nil, dtend: nil), calendar: cal) == nil)
        #expect(CalendarWeek.span(CalendarWeek.Event(uid: "y", summary: nil, dtstart: "next week", dtend: nil), calendar: cal) == nil)
    }

    @Test func theWindowIsTheLocalWeekAsInstants() {
        let cal = Self.calendar("Asia/Tokyo")
        let w = CalendarWeek.window(monday: Self.day("2026-11-02", cal), calendar: cal)
        #expect(w.start == "2026-11-01T15:00:00Z")
        #expect(w.end == "2026-11-08T15:00:00Z")
    }
}
