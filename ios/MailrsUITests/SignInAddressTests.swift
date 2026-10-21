import XCTest

/// The sign-in address must be a whole address before it is sent: half of
/// one is said on the spot rather than answered by the server as a wrong
/// password.
final class SignInAddressTests: MailrsUITestCase {

    func testAPartialAddressIsSaidAndCannotBeSent() {
        let app = launch()
        let address = app.textFields["you@example.com"]
        XCTAssertTrue(address.waitForExistence(timeout: 10), "sign-in form never appeared")
        address.tap()
        if let kept = address.value as? String, !kept.isEmpty, kept != "you@example.com" {
            address.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: kept.count))
        }
        let password = app.secureTextFields["Password"]
        let hint = app.staticTexts["sign-in-address-hint"]
        let signIn = app.buttons["Sign in"]

        address.typeText("me@golia")
        password.tap()
        password.typeText("hunter2")
        XCTAssertTrue(hint.waitForExistence(timeout: 5), "no hint for a partial address")
        XCTAssertFalse(signIn.isEnabled, "a partial address could be sent")

        address.tap()
        address.typeText(".jp")
        XCTAssertFalse(hint.waitForExistence(timeout: 2), "the hint stayed on a whole address")
        XCTAssertTrue(signIn.isEnabled, "a whole address could not be sent")
    }
}
