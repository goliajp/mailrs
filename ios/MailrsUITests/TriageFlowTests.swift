import XCTest

/// Moving a thread between buckets, and what a refused request does.
///
/// Both come from the phone. The server has had `mark-notification`,
/// `mark-promotion` and `move-to-inbox` since before this client
/// existed and the client reached none of them; and a refused request
/// used to throw the reader back to the sign-in screen, which is the
/// one thing that must not happen to someone who is signed in.
final class TriageFlowTests: MailrsUITestCase {

    /// Asserted at the wire, not on the screen: the row leaves the list
    /// optimistically either way, so a disappearing row proves nothing
    /// about which verb was sent.
    func testMarkingANotificationPostsThatVerb() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15), "inbox never listed")

        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        let item = app.buttons["Mark as notification"]
        XCTAssertTrue(longPress(row, until: item), "no bucket in the row menu")
        item.tap()

        var sent: [String] = []
        for _ in 0..<20 {
            sent = postedVerbs()
            if !sent.isEmpty { break }
            Thread.sleep(forTimeInterval: 0.25)
        }
        XCTAssertEqual(sent.first, "mark-notification t1", "wrong verb: \(sent)")
    }

    /// A refused request stays inside the mailbox.
    ///
    /// `state = .failed` used to render `SignInView`, because RootView
    /// shows it for every state that is not `.signedIn` — so a refused
    /// star logged you out and printed the error on the login form as a
    /// red button.
    func testARefusedRequestBannersRatherThanSigningOut() {
        let app = launch(signedIn: true)
        // After the launch, not before: `launch` resets the stub, so a
        // refusal armed first is wiped by the very next call.
        refuseVerb("archive")
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15), "inbox never listed")

        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        // The archive swipe, through the helper every other test uses —
        // hand-rolling the gesture is how the first attempt found no
        // button to tap and proved nothing.
        swipeAndTap(app, row: row, edge: .trailing, action: "Archive")

        XCTAssertTrue(
            app.descendants(matching: .any)["error-banner"].waitForExistence(timeout: 10),
            "no banner over the mailbox")
        // And still in the mailbox: the sign-in form has this field and
        // the list does not.
        XCTAssertFalse(app.secureTextFields["Password"].exists,
                       "a refused request threw the reader back to sign-in")
    }

}
