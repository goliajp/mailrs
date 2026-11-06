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
        // By identifier, not by label: the walk runs in Japanese and
        // Chinese too, where these say 選択 and 完了, and an English label
        // stopped it at the first step.
        app.buttons["list.select"].tap()
        app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch.tap()
        shoot(app, "01b-selection")
        app.buttons["list.done"].tap()

        // The move choice, from the swipe it now lives on.
        let target = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        swipeAndTap(app, row: target, edge: .leading, action: "swipe.move")
        XCTAssertTrue(app.buttons["move.notification"].waitForExistence(timeout: 5), "no move choice")
        shoot(app, "01c-move")
        app.buttons["move.cancel"].firstMatch.tap()

        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no tappable row")
        row.tap()
        // the folded first message, which says the same in every language
        XCTAssertTrue(app.buttons["collapsed-1"].firstMatch.waitForExistence(timeout: 10), "thread never opened")
        shoot(app, "02-thread")

        // Scrolled, because the message card's own header is what the
        // accessibility fix changed and it sits under the thread's.
        app.swipeUp()
        shoot(app, "03-thread-scrolled")

        // The invitation, which is on the second message. Photographed
        // because the card's whole job is to be read at a glance, and
        // no assertion can say whether it is.
        if app.staticTexts["Product sync"].waitForExistence(timeout: 10) {
            shoot(app, "03b-invite")
        }

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
    /// The lists and the row menu that filing added: the list menu with
    /// the labels, the Trash and its menu, and the inbox row's menu with
    /// the labels opened — in whichever language the run asks for, which
    /// is where a long Japanese action name would run out of room.
    func testPhotographsFiling() {
        let app = launch(signedIn: true, language: language, appearance: appearance)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15),
            "inbox never listed"
        )
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        let labels = app.buttons["row.menu.label"]
        if longPress(row, until: labels) {
            shoot(app, "20-row-menu")
            labels.tap()
            _ = app.buttons["row.menu.label.1"].waitForExistence(timeout: 5)
            shoot(app, "21-row-labels")
            app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.08)).tap()
        }
        app.buttons["open-lists"].tap()
        _ = app.buttons["list.label.1"].waitForExistence(timeout: 5)
        shoot(app, "22-lists")
        let trash = app.buttons[trashTitle]
        if trash.waitForExistence(timeout: 5) {
            trash.tap()
            let doomed = app.buttons.containing(
                NSPredicate(format: "label CONTAINS %@", "In the Trash")
            ).firstMatch
            if doomed.waitForExistence(timeout: 10) {
                shoot(app, "23-trash")
                if longPress(doomed, until: app.buttons["row.menu.deleteForever"]) {
                    shoot(app, "24-trash-menu")
                }
            }
        }
    }

    /// The verdict panel, the sender checks, the flagged link and its question.
    func testVerdictPanelAndLinkWarning() {
        let app = launch(signedIn: true, language: language, appearance: appearance)
        armVerdicts()
        openOrdinaryThread(app)
        let panel = element(app, "verdict-panel")
        _ = panel.waitForExistence(timeout: 15)
        shoot(app, "30-verdict-panel")
        let flagged = app.webViews.links["www.golia.jp/login"]
        if flagged.waitForExistence(timeout: 10) {
            bringIntoView(app, flagged, named: "the flagged link")
            shoot(app, "31-link-warning")
            flagged.tap()
            _ = app.alerts.firstMatch.waitForExistence(timeout: 5)
            shoot(app, "32-link-question")
        }
    }

    /// The Trash's name in the run's language, to find it in the menu.
    private var trashTitle: String {
        switch language {
        case "ja": return "ゴミ箱"
        case "zh-Hans": return "废纸篓"
        default: return "Trash"
        }
    }

    private func shoot(_ app: XCUIApplication, _ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}
