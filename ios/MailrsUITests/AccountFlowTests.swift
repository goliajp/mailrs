import XCTest

/// Connecting a mailbox somewhere else.
///
/// The screen exists on all three clients and had no UI test on any of
/// them until the stub could answer `/api/accounts/external` — which
/// meant the account list, the reason a broken one gives, and the
/// manual server form were all only ever exercised by hand.
final class AccountFlowTests: MailrsUITestCase {
    private func openAccounts(_ app: XCUIApplication) {
        XCTAssertTrue(app.buttons["Lists"].waitForExistence(timeout: 10), "no Lists button")
        app.buttons["Lists"].tap()
        XCTAssertTrue(app.buttons["Settings"].waitForExistence(timeout: 10), "no Settings row")
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Mail accounts")
        XCTAssertTrue(
            app.buttons["Mail accounts"].waitForExistence(timeout: 10),
            "settings never showed the mail accounts row")
        app.buttons["Mail accounts"].tap()
        XCTAssertTrue(
            app.staticTexts["Connect an account"].waitForExistence(timeout: 10),
            "the accounts sheet never opened")
    }

    func testListsTheConnectedMailboxes() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        openAccounts(app)

        // The row carries the identifier; whether its texts are
        // separately addressable depends on whether SwiftUI merged
        // them, which is what this tells apart from "nothing arrived".
        let row = app.descendants(matching: .any)["account.acc_gmail"]
        XCTAssertTrue(row.waitForExistence(timeout: 10),
                      "the account list never decoded")
        XCTAssertTrue(app.descendants(matching: .any)["account.acc_qq"].exists,
                      "a second account is missing")
    }

    /// The reason, on the screen somebody reads. The web had it in a
    /// hover tooltip and both phones had it nowhere.
    func testABrokenAccountSaysWhy() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        openAccounts(app)

        let why = app.descendants(matching: .any)["account.why.acc_qq"]
        XCTAssertTrue(why.waitForExistence(timeout: 10),
                      "an account that stopped syncing said nothing about it")
        XCTAssertTrue(why.label.contains("app password"),
                      "the reason shown was not the server's: \(why.label)")
    }

    /// Autodiscovery covers the providers people use; a company server
    /// with no SRV record and no ISPDB entry needs the boxes. They were
    /// in the API from the first day and no client ever sent them.
    func testTypingTheServersInSendsThem() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        openAccounts(app)

        let email = app.textFields["account.email"]
        XCTAssertTrue(email.waitForExistence(timeout: 10), "no form to fill")
        email.tap()
        email.typeText("me@internal.example.jp")

        let secret = app.secureTextFields["account.secret"]
        XCTAssertTrue(secret.waitForExistence(timeout: 10), "no secret field")
        secret.tap()
        secret.typeText("hunter2")

        // Shut by default: a form that opens with eight empty boxes
        // teaches everybody that connecting mail is hard.
        // The keyboard is up after the secret field, and the toggle
        // is behind it.
        // Not the navigation bar's "Done" — that one dismisses the
        // whole sheet.
        // Dismiss the keyboard by tapping the title — not the bar's
        // "Done", which dismisses the whole sheet.
        app.navigationBars["Mail accounts"].firstMatch.tap()
        let manual = app.buttons["account.manual"]
        XCTAssertTrue(manual.waitForExistence(timeout: 5), "no way to type the servers in")
        // Existing is not the same as reachable: the keyboard is up
        // after the secret field and covers the bottom of the form.
        for _ in 0..<6 where !manual.isHittable { app.swipeUp() }
        XCTAssertTrue(manual.isHittable, "the toggle is under the keyboard")
        XCTAssertFalse(app.textFields["account.incoming.host"].exists,
                       "the boxes were open before anybody asked for them")
        manual.tap()

        let host = app.textFields["account.incoming.host"]
        XCTAssertTrue(host.waitForExistence(timeout: 5), "the boxes never opened")
        host.tap()
        host.typeText("imap.internal.example.jp")
        let port = app.textFields["account.incoming.port"]
        port.tap()
        port.typeText("993")
    }
}
