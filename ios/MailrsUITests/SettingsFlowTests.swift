import XCTest

/// The reader's own settings: each change reaches the stub as the request
/// the server takes, and what the page shows is read back from it. The
/// same journeys as Android's `SettingsFlowTest`.
final class SettingsFlowTests: MailrsUITestCase {

    private func openPage(_ app: XCUIApplication, _ id: String) {
        tapInListsMenu(app, "open-settings")
        let link = element(app, id)
        scrollTo(app, element: link, named: id)
        link.tap()
    }

    /// Every write the stub took, oldest first.
    private func writes() -> [[String: Any]] {
        guard let url = URL(string: "http://localhost:6039/debug/settings") else { return [] }
        var result: [[String: Any]] = []
        let done = expectation(description: "debug/settings")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data, let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
                result = list
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }

    private func lastWrite(_ method: String, _ path: String) -> [String: Any]? {
        writes().last { ($0["method"] as? String) == method && ($0["path"] as? String) == path }
    }

    private func waitForWrite(_ method: String, _ path: String) -> [String: Any]? {
        for _ in 0..<20 {
            if let w = lastWrite(method, path) { return w }
            Thread.sleep(forTimeInterval: 0.25)
        }
        return nil
    }

    func testTheNotificationModeAndQuietHoursAreSaved() {
        let app = launch(signedIn: true)
        openPage(app, "settings.notifications")
        XCTAssertTrue(element(app, "notify.sender.news@shop.test").waitForExistence(timeout: 10),
                      "the sender set apart from the rule was not listed")
        XCTAssertTrue(element(app, "notify.thread.t1").exists, "the muted conversation was not listed")

        element(app, "notify.mode").tap()
        app.buttons["All mail except Junk, notifications and promotions"].tap()
        let mode = waitForWrite("PUT", "/api/notify")?["body"] as? [String: Any]
        XCTAssertEqual(mode?["mode"] as? String, "all", "the mode did not reach the server")

        element(app, "notify.quiet").switches.firstMatch.tap()
        Thread.sleep(forTimeInterval: 1)
        let quiet = lastWrite("PUT", "/api/notify")?["body"] as? [String: Any]
        XCTAssertEqual(quiet?["quiet_start"] as? Int, 22 * 60, "quiet hours did not start at 22:00")
        XCTAssertEqual(quiet?["quiet_end"] as? Int, 7 * 60, "quiet hours did not end at 07:00")
        XCTAssertEqual(quiet?["mode"] as? String, "all", "saving quiet hours lost the mode just chosen")
    }

    func testOtherDevicesAreSignedOut() {
        let app = launch(signedIn: true)
        openPage(app, "settings.sessions")
        XCTAssertTrue(element(app, "session.s-laptop").waitForExistence(timeout: 10), "the other device was not listed")
        XCTAssertTrue(element(app, "session.s-this").exists, "this device was not listed")
        element(app, "sessions.endOthers").tap()
        XCTAssertNotNil(waitForWrite("POST", "/api/auth/sessions/end-others"), "signing the others out never reached the server")
        XCTAssertTrue(element(app, "session.s-laptop").waitForNonExistence(timeout: 10), "the signed-out device stayed listed")
        XCTAssertTrue(element(app, "session.s-this").exists, "this device was signed out with the others")
    }

    func testAnAppPasswordIsRevokedAndNewOnesAreMadeOnTheWeb() {
        let app = launch(signedIn: true)
        openPage(app, "settings.appPasswords")
        let old = element(app, "appPassword.ap-old")
        XCTAssertTrue(old.waitForExistence(timeout: 10), "the app passwords were not listed")
        XCTAssertTrue(element(app, "appPasswords.web").exists, "no way to the web to make one")
        old.swipeLeft()
        app.buttons["Revoke"].tap()
        XCTAssertNotNil(waitForWrite("DELETE", "/api/auth/app-passwords/ap-old"), "the revoke never reached the server")
        XCTAssertTrue(old.waitForNonExistence(timeout: 10), "the revoked password stayed listed")
        XCTAssertTrue(element(app, "appPassword.ap-tb").exists, "revoking one took another with it")
    }

    func testTheVacationReplyIsTurnedOnWithItsTextKept() {
        let app = launch(signedIn: true)
        openPage(app, "settings.vacation")
        let toggle = element(app, "vacation.enabled")
        XCTAssertTrue(toggle.waitForExistence(timeout: 10), "the vacation reply never loaded")
        toggle.switches.firstMatch.tap()
        let body = waitForWrite("PUT", "/api/settings/vacation")?["body"] as? [String: Any]
        XCTAssertEqual(body?["enabled"] as? Bool, true, "turning it on did not reach the server")
        XCTAssertEqual(body?["body"] as? String, "Back on Monday.", "the reply's text was not sent back as read")
        XCTAssertEqual(body?["days"] as? Int, 7, "the reply's interval was not sent back as read")
    }

    func testAMaskedAddressIsCreatedAndListed() {
        let app = launch(signedIn: true)
        openPage(app, "settings.masked")
        XCTAssertTrue(element(app, "mask.m1").waitForExistence(timeout: 10), "the masked addresses were not listed")
        let site = element(app, "masked.forDomain")
        site.tap()
        site.typeText("news.test")
        element(app, "masked.create").tap()
        let body = waitForWrite("POST", "/api/masked-emails")?["body"] as? [String: Any]
        XCTAssertEqual(body?["forDomain"] as? String, "news.test", "the site did not reach the server")
        XCTAssertTrue(element(app, "mask.m2").waitForExistence(timeout: 10), "the new address was not listed")
    }

    /// "Never load" takes the Load images button away; "Ask each time" brings it back.
    func testImagesFollowTheSetting() {
        let app = launch(signedIn: true)
        openPage(app, "settings.images")
        app.buttons["Never load"].tap()
        app.navigationBars.buttons["Done"].tap()
        openOlderMessage(app)
        XCTAssertTrue(app.staticTexts["To: me@golia.jp"].waitForExistence(timeout: 10), "the older message did not unfold")
        XCTAssertFalse(element(app, "load-images").waitForExistence(timeout: 3), "Never load still offered Load images")
    }

    /// The other half: the same message offers the button by default, so
    /// its absence above is the setting's doing.
    func testImagesAskEachTimeByDefault() {
        let app = launch(signedIn: true)
        openOlderMessage(app)
        XCTAssertTrue(element(app, "load-images").waitForExistence(timeout: 10), "Ask each time offered no Load images")
    }

    func testASendersMailIsAlwaysFiledToPromotions() {
        let app = launch(signedIn: true)
        openOlderMessage(app)
        let sender = app.staticTexts["Alice Smith"].firstMatch
        XCTAssertTrue(sender.waitForExistence(timeout: 10), "the older message's sender was not shown")
        sender.press(forDuration: 1.0)
        element(app, "sender.file").tap()
        element(app, "sender.file.promotions").tap()
        let body = waitForWrite("POST", "/api/senders/reclassify")?["body"] as? [String: Any]
        XCTAssertEqual(body?["bucket"] as? String, "promotions", "the bucket did not reach the server")
        XCTAssertEqual(body?["sender"] as? String, "alice@example.com", "the sender's address did not reach the server")
    }

    func testAConversationIsMuted() {
        let app = launch(signedIn: true)
        openQuarterlyReport(app)
        app.buttons["More"].firstMatch.tap()
        element(app, "thread.notify").tap()
        element(app, "thread.notify.mute").tap()
        let body = waitForWrite("PUT", "/api/notify/threads/t1")?["body"] as? [String: Any]
        XCTAssertEqual(body?["value"] as? String, "mute", "muting did not reach the server")
    }

    private func openQuarterlyReport(_ app: XCUIApplication) {
        let row = app.buttons.containing(NSPredicate(format: "label CONTAINS %@", "Quarterly report")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
    }

    /// Alice's message, the one with a remote picture, opens folded.
    private func openOlderMessage(_ app: XCUIApplication) {
        openQuarterlyReport(app)
        let folded = app.buttons["collapsed-1"]
        XCTAssertTrue(folded.waitForExistence(timeout: 10), "the older message was not there to unfold")
        folded.tap()
    }
}
