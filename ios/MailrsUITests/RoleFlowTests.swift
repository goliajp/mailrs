import XCTest

/// Settings offers the pages the server would answer for this account: the
/// directory to an administrator, and the pages that show other people's
/// mail traffic — the queue and the audit log — to a super administrator
/// only. The account's rights are read again when the app comes back to the
/// front and after a refusal, so a right taken away hides its page.
final class RoleFlowTests: MailrsUITestCase {
    private func role(_ name: String) {
        guard let url = URL(string: "http://localhost:6039/debug/role/\(name)") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let done = expectation(description: "role")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }

    /// How many times the app asked who it is: what shows it asked again.
    private func meReads() -> Int {
        guard let url = URL(string: "http://localhost:6039/debug/role-state") else { return -1 }
        var result = -1
        let done = expectation(description: "role-state")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                result = json["me_reads"] as? Int ?? -1
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }

    private func waitForMeReads(atLeast n: Int, _ why: String) {
        let read = expectation(for: NSPredicate { _, _ in self.meReads() >= n }, evaluatedWith: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [read], timeout: 15), .completed, why)
    }

    /// Launched as `name`. Launching resets the stub to a super
    /// administrator, so the role is set after and the app started again.
    private func launched(as name: String) -> XCUIApplication {
        let app = launch(signedIn: true)
        role(name)
        let reads = meReads()
        app.terminate()
        app.launch()
        waitForMeReads(atLeast: reads + 1, "the app never asked who it is")
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        return app
    }

    private func openSettings(_ app: XCUIApplication) {
        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        scrollTo(app, button: "API keys")
    }

    private func scrolledToEnd(_ app: XCUIApplication) {
        for _ in 0..<4 { app.swipeUp() }
    }

    func testAPlainAccountIsOfferedNoOperatorPage() {
        let app = launched(as: "plain")
        openSettings(app)
        scrolledToEnd(app)
        XCTAssertTrue(app.buttons["Sign out"].exists, "settings did not scroll to its end")
        for name in ["Accounts", "Aliases", "Domains", "Queue", "DMARC", "Audit log"] {
            XCTAssertFalse(app.buttons[name].exists, "\(name) offered to an account that is not an administrator")
        }
    }

    func testAnAdministratorIsNotOfferedOtherPeoplesMailTraffic() {
        let app = launched(as: "admin")
        openSettings(app)
        scrollTo(app, button: "DMARC")
        XCTAssertTrue(app.buttons["Accounts"].exists || app.buttons["Aliases"].exists,
                      "the directory was not offered to an administrator")
        for name in ["Queue", "Audit log"] {
            XCTAssertFalse(app.buttons[name].exists, "\(name) offered to an administrator")
        }
    }

    func testARightTakenAwayHidesItsPageAfterARefusal() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        waitForMeReads(atLeast: 1, "the app never asked who it is")
        openSettings(app)
        scrollTo(app, button: "Queue")
        let reads = meReads()
        role("admin")
        app.buttons["Queue"].tap()
        // the 403 sends the app back to ask who it is
        waitForMeReads(atLeast: reads + 1, "a refusal did not make the app ask again")
        app.navigationBars["Queue"].buttons["Done"].tap()
        scrollTo(app, button: "DMARC")
        XCTAssertTrue(app.buttons["Queue"].waitForNonExistence(timeout: 10),
                      "the queue is still offered after the right was taken away")
    }

    func testARightTakenAwayHidesItsPageWhenTheAppComesBack() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        waitForMeReads(atLeast: 1, "the app never asked who it is")
        let reads = meReads()
        role("admin")
        XCUIDevice.shared.press(.home)
        app.activate()
        waitForMeReads(atLeast: reads + 1, "coming back did not make the app ask again")
        openSettings(app)
        scrollTo(app, button: "DMARC")
        XCTAssertFalse(app.buttons["Queue"].exists, "the queue is still offered after the right was taken away")
    }
}
