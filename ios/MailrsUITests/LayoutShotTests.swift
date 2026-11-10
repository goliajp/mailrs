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
        // the row's text: in select mode a row is a selectable cell, not
        // a link, so it is no longer a button
        app.staticTexts["Quarterly report and the follow-up notes"].tap()
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
            // the message's own actions, on the button in its header
            let more = app.buttons["message.more"].firstMatch
            if more.waitForExistence(timeout: 5) {
                more.tap()
                _ = app.buttons["View source"].waitForExistence(timeout: 5)
                shoot(app, "08-message-menu")
                app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.95)).tap()
            }
            // a long press on Load images is a press on the button, not
            // a menu over a preview of the whole card
            let load = app.buttons["load-images"].firstMatch
            if load.exists {
                load.press(forDuration: 1.2)
                shoot(app, "09-load-images-long-press")
            }
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

    /// Switching lists while the first page is out: what the screen shows
    /// before the rows arrive, for a list never opened and for one opened
    /// before, and the list once they are in.
    func testPhotographsListSwitch() {
        let app = launch(signedIn: true, language: language, appearance: appearance)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15),
            "inbox never listed"
        )
        shoot(app, "40-inbox")
        setStubListDelay(2500)
        tapInListsMenu(app, "Starred")
        Thread.sleep(forTimeInterval: 0.6)
        shoot(app, "41-starred-loading")
        _ = app.staticTexts["Starred thread"].waitForExistence(timeout: 10)
        shoot(app, "42-starred")
        tapInListsMenu(app, "Inbox")
        Thread.sleep(forTimeInterval: 0.6)
        shoot(app, "43-inbox-again-loading")
        Thread.sleep(forTimeInterval: 3)
        shoot(app, "44-inbox-again")
        setStubListDelay(0)
    }

    /// Every other surface a reader meets, for the design pass: row
    /// gestures, the lists menu, other lists and an empty one, search,
    /// drafts, the calendar and the reply sheet. English labels, so it
    /// is run in English, in both themes.
    func testPhotographsEverySurface() {
        let app = launch(signedIn: true, appearance: appearance)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15),
            "inbox never listed"
        )
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch

        row.swipeLeft()
        shoot(app, "50-swipe-trailing")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.6)).tap()
        row.swipeRight()
        shoot(app, "51-swipe-leading")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.6)).tap()

        if longPress(row, until: app.buttons["row.menu.label"]) {
            shoot(app, "52-row-menu")
            app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.08)).tap()
        }

        app.buttons["open-lists"].tap()
        Thread.sleep(forTimeInterval: 0.8)
        shoot(app, "53-lists-menu")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.9)).tap()

        for (name, key) in [("Sent", "54-sent"), ("Junk", "55-junk"), ("Snoozed", "56-empty")] {
            tapInListsMenu(app, name)
            Thread.sleep(forTimeInterval: 1.5)
            shoot(app, key)
        }
        tapInListsMenu(app, "Inbox")
        _ = row.waitForExistence(timeout: 10)

        let field = app.searchFields.firstMatch
        if field.waitForExistence(timeout: 5) {
            field.tap()
            Thread.sleep(forTimeInterval: 0.8)
            shoot(app, "57-search-empty")
            field.typeText("report")
            Thread.sleep(forTimeInterval: 1.5)
            shoot(app, "58-search-results")
            // iOS 26 closes the search field with an "x" beside it
            if app.buttons["close"].exists { app.buttons["close"].tap() }
            Thread.sleep(forTimeInterval: 0.8)
        }

        tapInListsMenu(app, "Drafts")
        Thread.sleep(forTimeInterval: 1.5)
        shoot(app, "59-drafts")
        app.swipeDown(velocity: .fast)
        Thread.sleep(forTimeInterval: 0.8)

        app.buttons["open-lists"].tap()
        scrollTo(app, button: "open-calendar")
        app.buttons["open-calendar"].tap()
        Thread.sleep(forTimeInterval: 1.5)
        shoot(app, "60-calendar")
        app.swipeDown(velocity: .fast)
        Thread.sleep(forTimeInterval: 0.8)

        _ = row.waitForExistence(timeout: 10)
        row.tap()
        if app.buttons["Reply"].waitForExistence(timeout: 10) {
            shoot(app, "61-thread-top")
            app.buttons["Reply"].tap()
            Thread.sleep(forTimeInterval: 1.2)
            shoot(app, "62-reply")
        }
    }

    func testPhotographsSignIn() {
        let app = launch(signedIn: false, language: language, appearance: appearance)
        Thread.sleep(forTimeInterval: 2)
        shoot(app, "70-sign-in")
    }

    /// The Trash's name in the run's language, to find it in the menu.
    private var trashTitle: String {
        switch language {
        case "ja": return "ゴミ箱"
        case "zh-Hans": return "已删除邮件"
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
