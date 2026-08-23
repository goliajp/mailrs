import XCTest

/// Adding a mailbox somewhere else.
///
/// A screen with no test on it is a screen exercised only by hand —
/// and the last time this app had one, three defects across three
/// platforms were hiding behind it. There is no stub for IMAP here, so
/// what is checked is everything up to the connection: that the form
/// says what this provider wants before anybody types it, that the
/// server boxes stay shut until asked for and open **filled in**, and
/// that a provider which refuses passwords says so rather than letting
/// somebody type one that cannot work.
final class MailboxFlowTests: MailrsUITestCase {
    private func openMailboxes(_ app: XCUIApplication) {
        XCTAssertTrue(app.buttons["Lists"].waitForExistence(timeout: 15), "no Lists button")
        app.buttons["Lists"].tap()
        XCTAssertTrue(app.buttons["Settings"].waitForExistence(timeout: 10), "no Settings row")
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Mailboxes")
        app.buttons["Mailboxes"].tap()
        XCTAssertTrue(
            app.textFields["account.address"].waitForExistence(timeout: 10),
            "the mailboxes sheet never opened")
    }

    /// Nothing is asked for until there is an address to ask about:
    /// a secret field over an empty address has nothing to label
    /// itself with.
    func testTheSecretIsNotAskedForUntilThereIsAnAddress() {
        let app = launch(signedIn: true)
        openMailboxes(app)
        XCTAssertFalse(
            app.secureTextFields["account.secret"].exists,
            "a secret was asked for before the address")

        let address = app.textFields["account.address"]
        address.tap()
        address.typeText("someone@qq.com")
        XCTAssertTrue(
            app.secureTextFields["account.secret"].waitForExistence(timeout: 5),
            "the secret field never appeared")
    }

    /// The provider's own word, and a link to make one. Typing a login
    /// password into a field labelled 授权码 is a mistake somebody
    /// recovers from; typing it into one labelled "Password" and being
    /// refused is not.
    func testAProviderThatWantsACodeSaysWhatItCallsIt() {
        let app = launch(signedIn: true)
        openMailboxes(app)
        let address = app.textFields["account.address"]
        address.tap()
        address.typeText("someone@qq.com")

        XCTAssertTrue(
            app.secureTextFields["授权码"].waitForExistence(timeout: 5),
            "the field was not labelled with the provider's own word")
        // SwiftUI's `Link` is a button to XCUITest, not a link.
        XCTAssertTrue(
            app.descendants(matching: .any)["Get one"].exists,
            "no way to go and make one")
    }

    /// Said at the start, rather than discovered at the end of a
    /// sign-in that could not have finished.
    func testAProviderThatRefusesPasswordsSaysSo() {
        let app = launch(signedIn: true)
        openMailboxes(app)
        let address = app.textFields["account.address"]
        address.tap()
        address.typeText("someone@gmail.com")

        XCTAssertTrue(
            app.descendants(matching: .any)["account.oauthUnavailable"]
                .waitForExistence(timeout: 5),
            "Gmail did not say it refuses passwords")
    }

    /// Shut until asked for — a form that opens with five empty boxes
    /// teaches everybody that connecting mail is hard — and then
    /// **filled in**, because an empty form is one somebody has to
    /// research and a filled one is one they correct.
    func testTheServerBoxesOpenFilledIn() {
        let app = launch(signedIn: true)
        openMailboxes(app)
        let address = app.textFields["account.address"]
        address.tap()
        address.typeText("someone@qq.com")

        XCTAssertTrue(app.buttons["account.manual"].waitForExistence(timeout: 5))
        XCTAssertFalse(
            app.textFields["account.incoming.host"].exists,
            "the boxes were open before anybody asked for them")
        app.buttons["account.manual"].tap()

        let host = app.textFields["account.incoming.host"]
        XCTAssertTrue(host.waitForExistence(timeout: 5), "the boxes never opened")
        XCTAssertEqual(
            host.value as? String, "imap.qq.com",
            "the boxes opened empty rather than filled in")
        XCTAssertEqual(app.textFields["account.incoming.port"].value as? String, "993")
    }
}
