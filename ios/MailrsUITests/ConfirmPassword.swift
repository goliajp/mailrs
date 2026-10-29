import XCTest

extension MailrsUITestCase {
    /// Answer the password prompt the server asks for before an
    /// administrative change. The stub accepts any password except
    /// `wrong-password`.
    func confirmPassword(_ app: XCUIApplication, password: String = "anything") {
        let field = app.secureTextFields["field.confirmPassword"]
        XCTAssertTrue(field.waitForExistence(timeout: 10),
                      "the change was not refused for want of a confirmation")
        field.tap()
        field.typeText(password)
        app.buttons["button.confirmPassword"].tap()
    }
}
