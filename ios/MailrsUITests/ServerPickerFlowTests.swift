import XCTest

/// The sign-in screen chooses its server from a saved list rather than
/// taking one typed by hand.
final class ServerPickerFlowTests: MailrsUITestCase {

    private func openServers(_ app: XCUIApplication) {
        let line = app.buttons["sign-in-server"]
        XCTAssertTrue(line.waitForExistence(timeout: 10), "the sign-in screen has no server line")
        line.tap()
        XCTAssertTrue(app.buttons["server.add"].waitForExistence(timeout: 5), "the server list never opened")
    }

    private func type(_ text: String, into id: String, in app: XCUIApplication, replacing: Bool = false) {
        let field = app.textFields[id]
        XCTAssertTrue(field.waitForExistence(timeout: 5), "no \(id) field")
        field.tap()
        if replacing, let kept = field.value as? String {
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: kept.count))
        }
        field.typeText(text)
    }

    private func addServer(_ app: XCUIApplication, name: String, address: String) {
        app.buttons["server.add"].tap()
        type(name, into: "server.field.name", in: app)
        type(address, into: "server.field.address", in: app)
        app.buttons["server.save"].tap()
        XCTAssertTrue(
            app.buttons["server.row.\(name)"].waitForExistence(timeout: 5), "the list never showed \(name)")
    }

    private func gone(_ element: XCUIElement) -> Bool {
        let absent = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: element)
        return XCTWaiter().wait(for: [absent], timeout: 5) == .completed
    }

    func testAFreshListOffersTheBuiltInServer() {
        let app = launch()
        openServers(app)
        let golia = app.buttons["server.row.GoliaMail"]
        XCTAssertTrue(golia.waitForExistence(timeout: 5), "no built-in server")
        XCTAssertTrue(golia.label.contains("mail.golia.ai"), "the row does not say where it is: \(golia.label)")
        XCTAssertFalse(app.buttons["server.hideBuiltIn"].exists, "the only server offered to hide itself")
    }

    /// The relaunch names a stub port nothing listens on, so the inbox
    /// can only have come from the server the list remembered.
    func testAnAddedServerIsChosenSignedInToAndRemembered() {
        let app = launch()
        openServers(app)
        addServer(app, name: "Stub", address: "http://localhost:6039")
        XCTAssertTrue(app.buttons["server.row.Stub"].isSelected, "a server just added is not the chosen one")
        app.buttons["server.done"].tap()
        let line = app.buttons["sign-in-server"]
        XCTAssertTrue(line.waitForExistence(timeout: 5))
        XCTAssertTrue(line.label.contains("Stub"), "the sign-in screen does not name the chosen server: \(line.label)")
        signIn(app)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"].waitForExistence(timeout: 15),
            "the chosen server never listed the inbox")
        app.terminate()

        let again = XCUIApplication()
        again.launchArguments = [
            "-mailrsBaseURL", "http://localhost:9", "-mailrsKeepServers", "-mailrsSignedOut",
            "-mailrsFreshCache", "-mailrs.language", "en", "-mailrs.remoteImages", "click",
            "-AppleLanguages", "(en)", "-AppleLocale", "en_US",
        ]
        again.launch()
        let remembered = again.buttons["sign-in-server"]
        XCTAssertTrue(remembered.waitForExistence(timeout: 10), "the sign-in screen never came back")
        XCTAssertTrue(
            remembered.label.contains("Stub"),
            "the server that signed in is not chosen after a relaunch: \(remembered.label)")
        signIn(again)
        XCTAssertTrue(
            again.staticTexts["Quarterly report and the follow-up notes"].waitForExistence(timeout: 15),
            "the remembered server never listed the inbox")
    }

    func testABadOrRepeatedAddressCannotBeSaved() {
        let app = launch()
        openServers(app)
        app.buttons["server.add"].tap()
        let save = app.buttons["server.save"]
        XCTAssertTrue(save.waitForExistence(timeout: 5), "the add form never opened")
        XCTAssertFalse(save.isEnabled, "an empty address could be saved")

        type("not a server", into: "server.field.address", in: app)
        let problem = app.staticTexts["server.problem"]
        XCTAssertTrue(problem.waitForExistence(timeout: 5), "nothing said the address was wrong")
        XCTAssertEqual(problem.label, "Enter a server address, like mail.example.com")
        XCTAssertFalse(save.isEnabled, "a bad address could be saved")

        type("https://mail.golia.ai/", into: "server.field.address", in: app, replacing: true)
        XCTAssertEqual(problem.label, "This server is already in the list")
        XCTAssertFalse(save.isEnabled, "a server already listed could be saved again")

        type("mail.example.com", into: "server.field.address", in: app, replacing: true)
        XCTAssertTrue(gone(problem), "the complaint stayed on a good address")
        XCTAssertTrue(save.isEnabled, "a good address could not be saved")

        app.buttons["server.cancel"].tap()
        XCTAssertTrue(app.buttons["server.add"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["server.row.mail.example.com"].exists, "a cancelled server was saved")
    }

    func testAnAddedServerIsRenamedAndDeleted() {
        let app = launch()
        openServers(app)
        addServer(app, name: "Office", address: "mail.example.com")

        app.buttons["server.edit.Office"].tap()
        let name = app.textFields["server.field.name"]
        XCTAssertTrue(name.waitForExistence(timeout: 5), "the edit form never opened")
        XCTAssertEqual(name.value as? String, "Office")
        XCTAssertEqual(app.textFields["server.field.address"].value as? String, "mail.example.com")
        type("Work", into: "server.field.name", in: app, replacing: true)
        app.buttons["server.save"].tap()
        let work = app.buttons["server.row.Work"]
        XCTAssertTrue(work.waitForExistence(timeout: 5), "the list never showed the new name")
        XCTAssertFalse(app.buttons["server.row.Office"].exists, "the old name stayed")
        XCTAssertTrue(work.isSelected, "a rename dropped the choice")

        app.buttons["server.edit.Work"].tap()
        let delete = app.buttons["server.delete"]
        XCTAssertTrue(delete.waitForExistence(timeout: 5), "the edit form has no delete")
        delete.tap()
        XCTAssertTrue(gone(work), "the deleted server stayed in the list")
        XCTAssertTrue(app.buttons["server.row.GoliaMail"].isSelected, "the choice did not fall back to a listed server")
    }

    func testTheBuiltInServerHidesAndReturns() {
        let app = launch()
        openServers(app)
        addServer(app, name: "Office", address: "mail.example.com")

        let golia = app.buttons["server.row.GoliaMail"]
        app.buttons["server.hideBuiltIn"].tap()
        XCTAssertTrue(gone(golia), "the hidden server stayed in the list")
        let restore = app.buttons["server.restore"]
        XCTAssertTrue(restore.waitForExistence(timeout: 5), "nothing offers the hidden server back")

        restore.tap()
        XCTAssertTrue(golia.waitForExistence(timeout: 5), "the built-in server did not come back")
        XCTAssertTrue(gone(restore), "the offer stayed after it was taken")
    }
}
