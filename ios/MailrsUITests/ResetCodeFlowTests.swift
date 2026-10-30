import XCTest

/// An account whose password an administrator cleared: signing in opens
/// the reset-code sheet, saying whom to ask; a wrong code is said as such;
/// the right one sets the new password, and the sign-in form says to sign
/// in with it. The sheet also opens without a failed sign-in.
final class ResetCodeFlowTests: MailrsUITestCase {
    private func resetDone() -> Bool {
        guard let url = URL(string: "http://localhost:6039/debug/reset-state") else { return false }
        var done = false
        let finished = expectation(description: "reset-state")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                done = json["done"] as? Bool ?? false
            }
            finished.fulfill()
        }.resume()
        wait(for: [finished], timeout: 10)
        return done
    }

    /// Type into a field, waiting for the keyboard first: keys sent while
    /// it is still coming up are lost, and a secure field shows nothing to
    /// check by eye. A secure field is checked by its length and typed again
    /// once if it came up short.
    private func type(_ app: XCUIApplication, _ field: XCUIElement, _ text: String) {
        for _ in 0..<2 {
            field.tap()
            _ = app.keyboards.firstMatch.waitForExistence(timeout: 5)
            if let kept = field.value as? String, !kept.isEmpty, kept != field.placeholderValue {
                field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: kept.count))
            }
            field.typeText(text)
            guard field.elementType == .secureTextField else { return }
            if (field.value as? String)?.count == text.count { return }
        }
    }

    func testSigningInToAClearedAccountLeadsToTheCode() {
        let app = launch()
        let address = app.textFields["you@example.com"]
        XCTAssertTrue(address.waitForExistence(timeout: 10), "sign-in form never appeared")
        type(app, address, "cleared@golia.jp")
        type(app, app.secureTextFields["Password"], "an-old-password")
        app.buttons["Sign in"].tap()

        let explain = app.staticTexts["text.resetExplain"]
        XCTAssertTrue(explain.waitForExistence(timeout: 10), "the reset sheet never opened")
        XCTAssertTrue(explain.label.contains("Li Hao (lihao@golia.jp)"), explain.label)
        XCTAssertEqual(app.textFields["field.resetAddress"].value as? String, "cleared@golia.jp")

        type(app, app.textFields["field.resetCode"], "WRONGCODE2")
        type(app, app.secureTextFields["field.resetPassword"], "a sound new passphrase")
        // shown as typed, so it can be checked before it is set
        app.switches["toggle.resetShowPassword"].switches.firstMatch.tap()
        XCTAssertEqual(app.textFields["field.resetPassword"].value as? String, "a sound new passphrase")
        app.buttons["button.resetSubmit"].tap()
        let failure = app.staticTexts["text.resetFailure"]
        XCTAssertTrue(failure.waitForExistence(timeout: 10), "a wrong code said nothing")
        XCTAssertTrue(failure.label.contains("Ask Li Hao (lihao@golia.jp) for a new code"), failure.label)
        XCTAssertFalse(resetDone())

        // typed as handed over: a dash and lower case
        type(app, app.textFields["field.resetCode"], "goodc-ode23")
        app.buttons["button.resetSubmit"].tap()
        XCTAssertTrue(app.staticTexts["text.resetDone"].waitForExistence(timeout: 10),
                      "the sign-in form did not say the password is set")
        XCTAssertTrue(resetDone(), "the reset never reached the server")
    }

    func testTheCodeSheetOpensWithoutAFailedSignIn() {
        let app = launch()
        let button = app.buttons["button.haveResetCode"]
        XCTAssertTrue(button.waitForExistence(timeout: 10), "no way to enter a code")
        button.tap()
        let explain = app.staticTexts["text.resetExplain"]
        XCTAssertTrue(explain.waitForExistence(timeout: 10), "the reset sheet never opened")
        XCTAssertTrue(explain.label.contains("an administrator"), explain.label)
    }
}
