import XCTest

/// The merged list of mail from connected mailboxes.
///
/// No mailbox is connected in a test run and there is no IMAP stub, so
/// what is checked is what a person meets first: that the row opens
/// something, that the something says what to do rather than showing
/// an empty screen, and that leaving it comes back to Settings.
final class MergedMailFlowTests: MailrsUITestCase {
    private func openMergedMail(_ app: XCUIApplication) {
        XCTAssertTrue(app.buttons["Lists"].waitForExistence(timeout: 15), "no Lists button")
        app.buttons["Lists"].tap()
        XCTAssertTrue(app.buttons["Settings"].waitForExistence(timeout: 10), "no Settings row")
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Other mail")
        app.buttons["Other mail"].tap()
    }

    /// With nothing connected the screen says where to go, rather than
    /// showing an empty list that reads as "your mail is gone".
    func testTheRowOpensAScreenThatSaysWhatToDo() {
        let app = launch(signedIn: true)
        openMergedMail(app)
        XCTAssertTrue(
            app.otherElements["mailboxes.empty"].waitForExistence(timeout: 10)
                || app.staticTexts["No mailboxes yet"].waitForExistence(timeout: 5),
            "the other-mail sheet never opened")
    }

    /// Done comes back to Settings, with the row still there — leaving
    /// a sub-screen must not leave the screen that owns it.
    func testDoneReturnsToSettings() {
        let app = launch(signedIn: true)
        openMergedMail(app)
        XCTAssertTrue(
            app.buttons["mailboxes.done"].waitForExistence(timeout: 10), "no Done button")
        app.buttons["mailboxes.done"].tap()
        XCTAssertTrue(
            app.buttons["Other mail"].waitForExistence(timeout: 10),
            "Done left settings altogether")
    }
}
