import XCTest

/// Why a message was held, and the links and numbers worth a second look.
///
/// The stub serves the verdict, the sender checks and the link warnings
/// only once `armVerdicts` asks, so the same thread unarmed is the
/// counter-example: no panel, no marks.
final class VerdictFlowTests: MailrsUITestCase {

    func testHeldMessageShowsItsLayersReasonsAndSenderChecks() {
        let app = launch(signedIn: true)
        armVerdicts()
        openOrdinaryThread(app)

        let panel = element(app, "verdict-panel")
        XCTAssertTrue(panel.waitForExistence(timeout: 15), "no verdict panel on a held message")
        XCTAssertTrue(app.staticTexts["Held: suspected fraud"].exists)
        // the rule's own sentence, with the name it carries
        let identity = element(app, "verdict-layer-identity")
        XCTAssertTrue(identity.exists, "no identity layer")
        XCTAssertTrue(identity.label.contains("The display name claims to be Golia."),
                      "rule reason not worded: \(identity.label)")
        // a content rule is a line of its own, with what it added
        let content = element(app, "verdict-layer-content")
        XCTAssertTrue(content.label.contains("The text asks you to send your own LINE QR code."),
                      "content rule not listed: \(content.label)")
        XCTAssertTrue(content.label.contains("+6.0"), "content rule's score not shown: \(content.label)")
        // a layer that passed with nothing to say, and one never checked
        XCTAssertTrue(element(app, "verdict-layer-provenance").label.contains("Nothing found."))
        XCTAssertTrue(element(app, "verdict-layer-transport").exists)
        XCTAssertTrue(app.staticTexts["rules 2026-11-03.1"].exists)

        let checks = element(app, "sender-checks")
        XCTAssertTrue(checks.exists, "no SPF/DKIM/DMARC line under a suspicious sender")
        for result in ["SPF: softfail", "DKIM: none", "DMARC: fail"] {
            XCTAssertTrue(checks.label.contains(result), "\(result) missing: \(checks.label)")
        }

        let release = element(app, "verdict-release")
        bringIntoView(app, release, named: "the release button")
        tapWhenSteady(release, named: "the release button")
        let deadline = Date().addingTimeInterval(10)
        while !postedVerbs().contains("release t1") && Date() < deadline {
            Thread.sleep(forTimeInterval: 0.3)
        }
        XCTAssertTrue(postedVerbs().contains("release t1"), "release never reached the server")
    }

    /// Junk reasons and other filing, for mail no layer found anything in.
    func testFiledMessageSaysWhyItIsThere() {
        let app = launch(signedIn: true)
        armVerdicts()
        openOrdinaryThread(app)
        // folded or not depends on what the reader has read; open it either way
        let collapsed = app.buttons["collapsed-1"]
        if collapsed.waitForExistence(timeout: 5) { collapsed.tap() }
        let note = element(app, "placement-note")
        XCTAssertTrue(note.waitForExistence(timeout: 10), "no placement note")
        XCTAssertTrue(note.label.contains("Promotions"), "class not named: \(note.label)")
    }

    /// The counter-example: almost every message has no verdict, and
    /// draws nothing.
    func testNoVerdictDrawsNothing() {
        let app = launch(signedIn: true)
        openOrdinaryThread(app)
        XCTAssertTrue(app.webViews.staticTexts["Short reply, narrow body."].waitForExistence(timeout: 15),
                      "the message never rendered")
        XCTAssertFalse(element(app, "verdict-panel").waitForExistence(timeout: 3),
                       "a panel was drawn for a message with no verdict")
        XCTAssertFalse(element(app, "sender-checks").exists)
        XCTAssertFalse(element(app, "callback-notice").exists)
    }

    /// A link that shows one host and goes to another asks before it
    /// opens; an ordinary link beside it does not.
    func testFlaggedLinkAsksFirstAndOrdinaryLinkDoesNot() {
        let app = launch(signedIn: true)
        armVerdicts()
        openOrdinaryThread(app)

        // drawn as an image beside the link, labelled with the reasons
        let mark = app.webViews.descendants(matching: .any).matching(
            NSPredicate(format: "label CONTAINS %@", "This link shows www.golia.jp")
        ).firstMatch
        XCTAssertTrue(mark.waitForExistence(timeout: 15), "no mark beside the flagged link")

        let notice = element(app, "callback-notice")
        XCTAssertTrue(notice.exists, "no callback notice")
        XCTAssertTrue(notice.label.contains("+81 3-1234-5678"), notice.label)

        let flagged = app.webViews.links["www.golia.jp/login"]
        bringIntoView(app, flagged, named: "the flagged link")
        tapWhenSteady(flagged, named: "the flagged link")
        let alert = app.alerts.firstMatch
        XCTAssertTrue(alert.waitForExistence(timeout: 10), "a flagged link opened without asking")
        XCTAssertTrue(alert.label.contains("webmail-oauth.wzldq.com")
                      || alert.staticTexts.containing(
                          NSPredicate(format: "label CONTAINS %@", "webmail-oauth.wzldq.com")).count > 0,
                      "the question does not say where it goes")
        alert.buttons["Cancel"].tap()
        XCTAssertTrue(alert.waitForNonExistence(timeout: 5))

        let ordinary = app.webViews.links["the help page"]
        bringIntoView(app, ordinary, named: "the ordinary link")
        tapWhenSteady(ordinary, named: "the ordinary link")
        // it went straight to the browser, which sends this app to the background
        XCTAssertTrue(app.wait(for: .runningBackground, timeout: 10),
                      "an ordinary link did not open")
        XCTAssertFalse(app.alerts.firstMatch.exists, "an ordinary link asked before opening")
        app.activate()
    }
}
