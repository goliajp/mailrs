import XCTest

/// A team conversation shows its internal notes and, when replying, who
/// else is on it; the calendar shows the week with an all-day event on its
/// own dates. The same journeys as Android's `TeamCalendarFlowTest`.
final class TeamCalendarFlowTests: MailrsUITestCase {

    private func openTeamConversation(_ app: XCUIApplication) {
        let reply = element(app, "section.reply")
        XCTAssertTrue(reply.waitForExistence(timeout: 10), "no section bar over the Inbox")
        reply.tap()
        let team = element(app, "row.conversation.sec_team")
        XCTAssertTrue(team.waitForExistence(timeout: 10), "the team conversation was not listed")
        team.tap()
    }

    func testATeamConversationShowsItsNotes() {
        let app = launch(signedIn: true)
        openTeamConversation(app)
        XCTAssertTrue(element(app, "team.note.1").waitForExistence(timeout: 10), "the first note was not shown")
        XCTAssertTrue(element(app, "team.note.2").exists, "the edited note was not shown")
        let note = app.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS 'Refund approved, waiting on finance'")).firstMatch
        XCTAssertTrue(note.exists, "a note's text was not shown")
    }

    func testReplyingSaysWhoElseIsWriting() {
        let app = launch(signedIn: true)
        openTeamConversation(app)
        let replyButton = app.buttons["Reply"]
        XCTAssertTrue(replyButton.waitForExistence(timeout: 10), "the team conversation never opened")
        replyButton.tap()
        let hint = element(app, "reply.claimHint")
        XCTAssertTrue(hint.waitForExistence(timeout: 10), "replying did not say who else is on it")
        XCTAssertTrue(hint.label.contains("carol"), "the hint did not name carol, who is writing: \(hint.label)")
    }

    func testTheWeekShowsAnAllDayEventOnItsOwnDates() {
        let app = launch(signedIn: true)
        let lists = app.buttons["Lists"]
        XCTAssertTrue(lists.waitForExistence(timeout: 15), "inbox never listed")
        lists.tap()
        let open = element(app, "open-calendar")
        XCTAssertTrue(open.waitForExistence(timeout: 5), "the menu offered no calendar")
        open.tap()

        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = .current
        let monday = startOfWeek(Date(), cal)
        func key(_ days: Int) -> String { dayKey(cal.date(byAdding: .day, value: days, to: monday)!, cal) }

        XCTAssertTrue(element(app, "calendar.day.\(key(0))").waitForExistence(timeout: 10),
                      "the calendar did not open on this week")
        // the stub sends the offsite as dates, Wednesday to Friday exclusive
        XCTAssertTrue(element(app, "calendar.event.ev-offsite.\(key(2))").waitForExistence(timeout: 10),
                      "the all-day event is missing on Wednesday")
        XCTAssertTrue(element(app, "calendar.event.ev-offsite.\(key(3))").exists, "the all-day event is missing on Thursday")
        XCTAssertFalse(element(app, "calendar.event.ev-offsite.\(key(4))").exists, "the all-day event ran into Friday")
        XCTAssertFalse(element(app, "calendar.event.ev-offsite.\(key(1))").exists, "the all-day event started on Tuesday")
        let noEnd = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH 'calendar.event.ev-open.'")).firstMatch
        XCTAssertTrue(noEnd.exists, "the event with no end was not shown")

        element(app, "calendar.next").tap()
        XCTAssertTrue(element(app, "calendar.day.\(key(7))").waitForExistence(timeout: 10),
                      "Next week did not move a week on")
    }

    private func startOfWeek(_ day: Date, _ cal: Calendar) -> Date {
        let midnight = cal.startOfDay(for: day)
        let sinceMonday = (cal.component(.weekday, from: midnight) + 5) % 7
        return cal.date(byAdding: .day, value: -sinceMonday, to: midnight)!
    }

    private func dayKey(_ day: Date, _ cal: Calendar) -> String {
        let c = cal.dateComponents([.year, .month, .day], from: day)
        return String(format: "%04d-%02d-%02d", c.year!, c.month!, c.day!)
    }
}
