import XCTest

@testable import Mailrs

/// The chip row shows one shape, whatever the writer typed.
///
/// From a 2026-08-21 report: a support reply carried eight chips
/// reading `Aug 21 2026`, `2026-08-20`, `2026-08-21`, in three
/// different formats, because each quoted the source text back.
final class DateChipTests: XCTestCase {
    private func s(_ date: String, _ datetime: String? = nil, _ text: String = "written")
        -> Wire.DateSuggestion
    {
        Wire.DateSuggestion(date: date, datetime: datetime, text: text)
    }

    func testThreeFormatsBecomeOneShape() {
        let labels = [
            DateChip.label(s("2026-08-21", nil, "Aug 21 2026")),
            DateChip.label(s("2026-08-20", nil, "2026-08-20")),
            DateChip.label(s("2026-08-19", nil, "2026-08-19")),
        ]
        // Normalise both digits and words: the day and the weekday
        // differ, of course. What must not differ is the pattern — and
        // an earlier draft of this test normalised only the digits,
        // which made three correct labels look like three formats.
        let shapes = Set(
            labels.map {
                $0.replacingOccurrences(of: "[0-9]+", with: "#", options: .regularExpression)
                    .replacingOccurrences(
                        of: "[\\p{L}]+", with: "X", options: .regularExpression)
            })
        XCTAssertEqual(shapes.count, 1, "the row still carries the writer's formats: \(labels)")
        XCTAssertEqual(Set(labels).count, 3, "three days collapsed into fewer labels")
    }

    /// A bare `YYYY-MM-DD` read as an instant is UTC midnight, and
    /// renders as the day before for any reader west of Greenwich.
    func testTheDayIsTheLocalDay() {
        XCTAssertTrue(DateChip.label(s("2026-08-21")).contains("21"))
    }

    func testAnHourShowsOnlyWhenOneWasWritten() {
        let withTime = DateChip.label(s("2026-08-25", "2026-08-25T14:00:00"))
        let allDay = DateChip.label(s("2026-08-25"))
        XCTAssertNotEqual(withTime, allDay)
        XCTAssertGreaterThan(withTime.count, allDay.count)
    }

    /// Unparseable input falls back rather than inventing a day.
    func testNonsenseFallsBack() {
        XCTAssertEqual(DateChip.label(s("not-a-date")), "written")
        XCTAssertEqual(DateChip.label(s("2026-08-25", "garbage")), DateChip.label(s("2026-08-25")))
    }
}
