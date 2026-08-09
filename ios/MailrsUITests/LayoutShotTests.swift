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

        // Select mode, with something chosen. Reported from the phone
        // as not being able to tell which rows were picked.
        app.buttons["Select"].tap()
        app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch.tap()
        shoot(app, "01b-selection")
        app.buttons["Done"].tap()

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

    /// Every real message the stub was given, opened and photographed.
    ///
    /// `MAILRS_STUB_REAL` points the stub at mail captured from a live
    /// mailbox — 600px marketing tables, CJK newsletters, cid images.
    /// The hand-written fixture is one 760px table; what a reader
    /// actually gets is more varied than that, and the width fix was
    /// verified against the fixture alone.
    func testPhotographsRealMail() {
        let app = launch(signedIn: true, appearance: appearance)
        guard app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15) else { return }

        // Matched on the subject, not the stub's key for it: the list
        // row draws sender, date and subject and no snippet, so the key
        // never reaches the accessibility label.
        let mail = [
            // Not "配送": that subject carries zero-width characters
            // between its glyphs — the preheader padding 4% of real
            // HTML mail uses — so a plain substring never matches.
            ("cid", "完了"), ("cjk", "お部屋探し"), ("darkaware", "Shu Wang"),
            ("pdf", "Alignment"), ("textonly", "Pixel-7"), ("wide", "OpenClaw"),
        ]
        for (key, subject) in mail {
            let row = app.buttons.containing(
                NSPredicate(format: "label CONTAINS %@", subject)
            ).firstMatch
            guard row.waitForExistence(timeout: 5) else { continue }
            row.tap()
            _ = app.staticTexts.containing(
                NSPredicate(format: "label BEGINSWITH %@", "To:")
            ).firstMatch.waitForExistence(timeout: 10)
            shoot(app, "real-\(key)")
            app.navigationBars.buttons.firstMatch.tap()
            _ = app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 10)
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
            // The sheet's own Done button, by identity. Waiting on the
            // signed-in address instead was language-independent but
            // still flaky — a `LabeledContent` value is not reliably a
            // queryable element while the sheet is settling, and the
            // run left a pile of debug attachments saying so.
            app.buttons["settings-done"].waitForExistence(timeout: 10),
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
