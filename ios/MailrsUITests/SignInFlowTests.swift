import XCTest

/// The path a person actually takes: sign in, see the inbox, open a
/// thread.
///
/// Against a stub on localhost rather than a live server — a test that
/// needs someone's real password is a test nobody runs. The stub serves
/// the shapes the Rust handlers send, including a 760px-wide HTML body,
/// so the fit-to-width path is exercised rather than asserted about.
final class SignInFlowTests: XCTestCase {
    private func launch(signedIn: Bool = false, folder: String? = nil) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["-mailrsBaseURL", "http://localhost:6039"]
        app.launchArguments += signedIn ? ["-mailrsToken", "stub-token"] : ["-mailrsSignedOut"]
        if let folder { app.launchArguments += ["-mailrsFolder", folder] }
        app.launch()
        return app
    }

    /// Types the credentials and taps through. Shared because both tests
    /// need to be signed in, and a helper that fails loudly beats two
    /// copies drifting apart.
    private func signIn(_ app: XCUIApplication) {
        let address = app.textFields["you@example.com"]
        XCTAssertTrue(address.waitForExistence(timeout: 10), "sign-in form never appeared")
        address.tap()
        address.typeText("me@golia.jp")
        let password = app.secureTextFields["Password"]
        password.tap()
        password.typeText("hunter2")
        app.buttons["Sign in"].tap()
        dismissPasswordPrompt()
    }

    /// iOS offers to save the password after a `SecureField` sign-in, and
    /// that prompt belongs to SpringBoard, not to this app — it sits over
    /// the inbox and every row underneath is genuinely untappable while
    /// it is up. Which is what "the row is not hittable" meant, after
    /// three wrong guesses at overlays and modifiers inside the app.
    ///
    /// The labels are localised, so this tries the ones this machine's
    /// simulators actually produce rather than assuming English.
    private func dismissPasswordPrompt() {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        // Any button that dismisses it. The dialog is localised — this
        // machine's simulator answers in Chinese — and matching on labels
        // alone was the guess that did not work, so this falls back to
        // "whatever buttons the alert has".
        for label in ["Not Now", "以后", "後で", "Later", "保存", "Save"] {
            let button = springboard.buttons[label]
            if button.waitForExistence(timeout: 2) {
                button.tap()
                return
            }
        }
        let alert = springboard.alerts.firstMatch
        if alert.waitForExistence(timeout: 2), alert.buttons.count > 0 {
            alert.buttons.element(boundBy: 0).tap()
            return
        }
        // Nothing matched — say what was actually there rather than
        // failing three screens later with an unrelated message.
        if springboard.buttons.count > 0 {
            print("=== SPRINGBOARD BUTTONS ===")
            print(springboard.debugDescription)
        }
    }

    func testSignsInAndListsTheInbox() {
        let app = launch()
        signIn(app)

        // The row is the assertion: reaching the list means the login
        // decoded, the token was kept, and the bare-array conversation
        // response decoded too.
        let firstRow = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(firstRow.waitForExistence(timeout: 15), "inbox never listed")
        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].exists,
                      "the CJK subject did not survive the round trip")
    }

    func testOpensAThreadAndRendersTheBody() {
        // Starts signed in: this test is about opening a thread, and
        // going through the login form summons the system's save-password
        // prompt over the inbox, which nothing here can reach.
        let app = launch(signedIn: true)

        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"].waitForExistence(timeout: 15)
        )
        // The row's tap target is a Button whose accessibility label is
        // the whole row read out — "alice@example.com, Aug 5, Quarterly
        // report…". Neither the cell nor the subject label is hittable on
        // its own, which the hierarchy dump settled after two guesses.
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no tappable row")
        row.tap()

        // "To: …" only exists on the thread screen. `alice@example.com`
        // was the first assertion here and it is also the sender shown in
        // the inbox row, so it passed without anything having navigated —
        // an assertion satisfied by the screen you were already on.
        XCTAssertTrue(app.staticTexts["To: me@golia.jp"].waitForExistence(timeout: 10),
                      "thread never opened")

        // The spoofed sender is the second message, below a body authored
        // at 760px — so it has to be scrolled to. The badge is the point
        // of the assertion: a forged From must not borrow the trust of a
        // verified one.
        let badge = app.staticTexts["Unverified sender"]
        var swipes = 0
        while !badge.exists && swipes < 8 {
            app.swipeUp()
            swipes += 1
        }
        XCTAssertTrue(badge.exists, "a suspicious sender was shown without saying so")
    }

    /// Paging must not drop the threads that share the boundary second.
    ///
    /// The stub's paging fixture puts five threads on one timestamp
    /// straddling the 50-row page edge, because the server filters
    /// `latest_date < before_ts` — so a client that asked for its oldest
    /// row's own second would lose the three that did not fit, silently,
    /// and a shorter list looks exactly like the end of the mailbox.
    /// `Paged thread 51` is one of the three.
    func testPagingDoesNotSkipTheBoundarySecond() {
        let app = launch(signedIn: true, folder: "Paged")
        XCTAssertTrue(app.staticTexts["Paged thread 0"].waitForExistence(timeout: 15),
                      "paged list never loaded")

        let target = app.staticTexts["Paged thread 51"]
        var swipes = 0
        while !target.exists && swipes < 40 {
            app.swipeUp(velocity: .fast)
            swipes += 1
        }
        XCTAssertTrue(target.exists, "a thread sharing the page-boundary second was skipped")
    }

    func testRepliesToAThread() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()

        XCTAssertTrue(app.staticTexts["To: me@golia.jp"].waitForExistence(timeout: 10),
                      "thread never opened")
        app.buttons["Reply"].tap()

        // The subject is prefilled from the thread and not editable here,
        // so the only thing to type is the message.
        let editor = app.textViews.firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 5), "no composer")
        editor.tap()
        editor.typeText("Noted, thanks.")

        app.buttons["Send"].tap()

        // Back on the thread: the sheet dismisses only on a send the
        // server said it queued. If it had failed the sheet would still
        // be up with the reason in it.
        XCTAssertTrue(app.staticTexts["To: me@golia.jp"].waitForExistence(timeout: 10),
                      "the reply sheet never dismissed — the send did not succeed")
    }
}
