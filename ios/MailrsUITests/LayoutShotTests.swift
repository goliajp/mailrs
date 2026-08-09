import XCTest

/// Screens, photographed, at whatever text size the run asks for.
///
/// Not assertions — a screenshot cannot fail. This exists because the
/// conversation row was broken at the accessibility sizes for the whole
/// life of the app and nothing said so: every test passed, because the
/// tests read labels and a label is still there when it renders as
/// "A…". The only instrument that sees it is an eye, and the only way
/// to get an eye onto the thread screen is to drive the app there.
///
/// Run through `scripts/ios-shots.sh`, which sets the size and pulls the
/// attachments back out of the result bundle. Skipped in the ordinary
/// suite — `MAILRS_SHOTS` is how the script says it means it — so a
/// normal run does not spend a minute taking pictures nobody asked for.
final class LayoutShotTests: MailrsUITestCase {

    /// The theme the shots run asks for, if it asked. `system` follows
    /// the simulator's own appearance, which is what the script sets.
    private var appearance: String? {
        ProcessInfo.processInfo.environment["MAILRS_SHOTS_APPEARANCE"]
    }

    /// The language the shots run asks for; the suite's own default is
    /// English so the walk finds its buttons.
    private var language: String? {
        ProcessInfo.processInfo.environment["MAILRS_SHOTS_LANGUAGE"]
    }

    override func setUpWithError() throws {
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment["MAILRS_SHOTS"] == "1",
            "screenshot lane — run scripts/ios-shots.sh"
        )
    }

    func testWalksTheReadingPathTakingPictures() {
        let app = launch(signedIn: true, language: language, appearance: appearance)

        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15),
            "inbox never listed"
        )
        shoot(app, "01-list")

        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no tappable row")
        row.tap()
        XCTAssertTrue(
            app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH %@", "To:"))
                .firstMatch.waitForExistence(timeout: 10),
            "thread never opened"
        )
        shoot(app, "02-thread")

        // Scrolled, because the message card's own header is what the
        // accessibility fix changed and it sits under the thread's.
        app.swipeUp()
        shoot(app, "03-thread-scrolled")

        // The wide message — a 760px newsletter table, which is the
        // shape most real mail has and the one the fit-to-width path
        // exists for. It opens folded, so no shot had ever contained
        // it until a reader said on the phone that wide mail arrives
        // needing a pinch.
        let folded = app.buttons["collapsed-1"].firstMatch
        if folded.waitForExistence(timeout: 5) {
            folded.tap()
            // Waited for, not slept past: the card animates open and
            // the body lands after its own measure pass, so the first
            // shot caught a half-expanded card with no message in it.
            // The images offer only appears once the body is up.
            _ = app.buttons["load-images"].waitForExistence(timeout: 10)
            shoot(app, "07-wide-body")
        }
    }

    func testPhotographsComposeAndSettings() {
        let app = launch(signedIn: true, language: language, appearance: appearance)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15),
            "inbox never listed"
        )

        app.buttons["new-message"].tap()
        XCTAssertTrue(
            app.textFields["composer-to"].waitForExistence(timeout: 10),
            "composer never opened"
        )
        shoot(app, "04-compose")
        app.buttons["composer-cancel"].tap()

        app.buttons["open-lists"].tap()
        // Scrolled to, not tapped blind: at the accessibility sizes the
        // sheet's rows are tall enough that Settings is below the fold,
        // and a List does not build a cell it has not shown.
        scrollTo(app, button: "open-settings")
        app.buttons["open-settings"].tap()
        XCTAssertTrue(
            // The signed-in address, which is data and not a word the
            // catalog translates — unlike "Signed in as", which this
            // waited on until the Japanese run could not find it.
            app.staticTexts["me@golia.jp"].waitForExistence(timeout: 10),
            "settings never opened"
        )
        shoot(app, "05-settings")
        app.swipeUp()
        shoot(app, "06-settings-scrolled")
    }

    /// `.keepAlways`, or the bundle throws away everything that passed —
    /// which is every run of this file.
    private func shoot(_ app: XCUIApplication, _ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}
