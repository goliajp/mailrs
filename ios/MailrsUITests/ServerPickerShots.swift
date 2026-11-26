import XCTest

/// Pictures of the server line, the saved list and its forms, taken by
/// the screenshot lane: `scripts/ios-shots.sh large light en testPhotographsServerPicker`.
extension LayoutShotTests {
    func testPhotographsServerPicker() {
        let environment = ProcessInfo.processInfo.environment
        let app = launch(
            signedIn: false, language: environment["MAILRS_SHOTS_LANGUAGE"],
            appearance: environment["MAILRS_SHOTS_APPEARANCE"])
        let line = app.buttons["sign-in-server"]
        XCTAssertTrue(line.waitForExistence(timeout: 10), "the sign-in screen has no server line")
        line.tap()
        XCTAssertTrue(app.buttons["server.row.GoliaMail"].waitForExistence(timeout: 5))
        app.buttons["server.row.GoliaMail"].tap()
        XCTAssertTrue(line.waitForExistence(timeout: 5))
        Thread.sleep(forTimeInterval: 1)
        picture(app, "80-sign-in-server")

        line.tap()
        for (name, address) in [("Office", "mail.example.com"), ("Home lab", "http://192.168.1.20:8080")] {
            app.buttons["server.add"].tap()
            let nameField = app.textFields["server.field.name"]
            XCTAssertTrue(nameField.waitForExistence(timeout: 5), "the add form never opened")
            nameField.tap()
            nameField.typeText(name)
            let addressField = app.textFields["server.field.address"]
            addressField.tap()
            addressField.typeText(address)
            if name == "Home lab" { picture(app, "82-add-server") }
            app.buttons["server.save"].tap()
            XCTAssertTrue(app.buttons["server.row.\(name)"].waitForExistence(timeout: 5))
        }
        Thread.sleep(forTimeInterval: 1)
        picture(app, "81-servers")

        app.buttons["server.edit.Office"].tap()
        let addressField = app.textFields["server.field.address"]
        XCTAssertTrue(addressField.waitForExistence(timeout: 5), "the edit form never opened")
        addressField.tap()
        addressField.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 16) + "mail.golia.ai")
        XCTAssertTrue(app.staticTexts["server.problem"].waitForExistence(timeout: 5))
        picture(app, "83-edit-server-refused")
        app.buttons.matching(identifier: "server.cancel").firstMatch.tap()

        app.buttons["server.hideBuiltIn"].tap()
        XCTAssertTrue(app.buttons["server.restore"].waitForExistence(timeout: 5))
        Thread.sleep(forTimeInterval: 1)
        picture(app, "84-built-in-hidden")
    }

    private func picture(_ app: XCUIApplication, _ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}
