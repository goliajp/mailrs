import XCTest

/// The invitation, on screen.
///
/// Everything under it was already proven: the server resolves the
/// instant, the thread response marks the message, the payload decodes.
/// None of that says the card is *mounted*, and a card that renders
/// nothing looks exactly like a message that carries no calendar part.
/// This is the assertion that can tell them apart.
final class InviteFlowTests: MailrsUITestCase {
    func testAnInvitationRendersItsCard() {
        let app = launch(signedIn: true)
        let row = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(
            app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH %@", "To:"))
                .firstMatch.waitForExistence(timeout: 10),
            "thread never opened")

        XCTAssertTrue(
            app.staticTexts["Product sync"].waitForExistence(timeout: 10),
            "the invitation's summary is not on screen — the card did not mount")

        // The badge that says this is a re-send rather than a first
        // invitation: Exchange does not send METHOD:UPDATE, it raises
        // SEQUENCE, and calling that "New invite" tells the reader the
        // opposite of what happened.
        XCTAssertTrue(app.staticTexts["Updated invite"].exists, "the badge does not say it is an update")

        // The whole timezone argument, in one label: 16:00 in Santa
        // Clara is 08:00 the next morning here, and the organiser's own
        // clock is named beside it because neither number alone is the
        // answer.
        let when = app.staticTexts.containing(
            NSPredicate(format: "label CONTAINS %@", "Pacific Standard Time")
        ).firstMatch
        XCTAssertTrue(when.waitForExistence(timeout: 5), "the organiser's zone is not named")
        XCTAssertTrue(
            when.label.contains("8:00") || when.label.contains("08:00"),
            "the reader's own time is wrong or missing: \(when.label)")

        XCTAssertTrue(app.buttons["invite.join"].exists, "no way to join the meeting")
        XCTAssertTrue(app.buttons["invite.accepted"].exists, "no way to answer it")
    }
}
