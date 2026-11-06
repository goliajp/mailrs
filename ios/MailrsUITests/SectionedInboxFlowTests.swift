import XCTest

/// The section tabs over the Inbox follow the reader's sectioned inbox
/// setting, the one the web and Android read too: on until it is turned
/// off, off in Settings saves on the server and takes the tabs away, and
/// off on the server keeps them away when the app opens. The same journeys
/// as Android's `SectionedInboxFlowTest`.
final class SectionedInboxFlowTests: MailrsUITestCase {

    func testTurningTheSettingOffHidesTheTabsAndSavesOnTheServer() {
        let app = launch(signedIn: true)
        XCTAssertTrue(element(app, "section.all").waitForExistence(timeout: 10),
                      "no section tabs over the Inbox while the setting is on")

        tapInListsMenu(app, "open-settings")
        let toggle = app.switches["settings.sectionedInbox"]
        scrollTo(app, element: toggle, named: "settings.sectionedInbox")
        XCTAssertEqual(toggle.value as? String, "1", "the switch is not on while the server says on")
        toggle.switches.firstMatch.tap()
        XCTAssertTrue(savedEnabled(is: false), "turning it off did not reach the server")
        app.navigationBars.buttons["Done"].tap()

        XCTAssertTrue(element(app, "section.all").waitForNonExistence(timeout: 10),
                      "the section tabs stayed after the setting was turned off")
    }

    func testOffOnTheServerKeepsTheTabsAwayWhenTheAppOpens() {
        let app = launch(signedIn: true)
        XCTAssertTrue(element(app, "section.all").waitForExistence(timeout: 10), "no section tabs at first")
        app.terminate()
        // turned off on another device
        putSetting(enabled: false)
        app.launch()
        XCTAssertTrue(element(app, "row.conversation.t1").waitForExistence(timeout: 15), "the Inbox never listed")
        XCTAssertTrue(element(app, "section.all").waitForNonExistence(timeout: 10),
                      "the tabs showed though the server says off")
    }

    /// Whether the stub's last sectioned inbox save set `enabled` to `on`.
    private func savedEnabled(is on: Bool) -> Bool {
        for _ in 0..<20 {
            if lastSave()?["enabled"] as? Bool == on { return true }
            Thread.sleep(forTimeInterval: 0.25)
        }
        return false
    }

    private func lastSave() -> [String: Any]? {
        guard let url = URL(string: "http://localhost:6039/debug/settings") else { return nil }
        var found: [String: Any]?
        let done = expectation(description: "debug/settings")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data, let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
                found = list.last { ($0["path"] as? String) == "/api/settings/sectioned-inbox" }?["body"] as? [String: Any]
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return found
    }

    private func putSetting(enabled: Bool) {
        guard let url = URL(string: "http://localhost:6039/api/settings/sectioned-inbox") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "PUT"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer stub-token", forHTTPHeaderField: "Authorization")
        request.httpBody = try? JSONSerialization.data(withJSONObject: ["enabled": enabled])
        let done = expectation(description: "put sectioned-inbox")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }
}
