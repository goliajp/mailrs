import XCTest

/// The Trash, Review, the extra lists and labels, as a reader reaches
/// them. Asserted at the wire where the screen alone would prove
/// nothing: a row leaves the list optimistically whatever was sent.
final class FilingFlowTests: MailrsUITestCase {

    private func waitForVerb(_ wanted: String) -> [String] {
        var sent: [String] = []
        for _ in 0..<40 {
            sent = postedVerbs()
            if sent.contains(wanted) { break }
            Thread.sleep(forTimeInterval: 0.25)
        }
        return sent
    }

    private func open(_ app: XCUIApplication, list: String) {
        app.buttons["Lists"].tap()
        app.buttons[list].tap()
    }

    private func inboxListed(_ app: XCUIApplication) {
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
    }

    private func row(_ app: XCUIApplication, _ text: String) -> XCUIElement {
        app.buttons.containing(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    /// Moving to the Trash does not ask, and the undo takes it back.
    func testTrashingAsksNothingAndUndoes() {
        let app = launch(signedIn: true)
        inboxListed(app)
        let item = app.buttons["row.menu.trash"]
        XCTAssertTrue(longPress(row(app, "Quarterly report"), until: item), "no Trash in the row menu")
        item.tap()
        XCTAssertFalse(app.alerts.firstMatch.waitForExistence(timeout: 1),
                       "moving to the Trash asked first")
        let undo = app.buttons["undo-archive"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5), "no undo offered")
        XCTAssertTrue(app.staticTexts["Moved to Trash"].exists, "the undo bar named another action")
        XCTAssertTrue(waitForVerb("delete t1").contains("delete t1"), "the Trash never reached the server")
        undo.tap()
        XCTAssertTrue(waitForVerb("untrash t1").contains("untrash t1"), "the undo did not restore it")
    }

    func testTheTrashRestores() {
        let app = launch(signedIn: true)
        inboxListed(app)
        open(app, list: "Trash")
        XCTAssertTrue(app.staticTexts["In the Trash: trash1"].waitForExistence(timeout: 10),
                      "the Trash did not ask for trashed conversations")
        let item = app.buttons["row.menu.untrash"]
        XCTAssertTrue(longPress(row(app, "In the Trash"), until: item), "no Restore in the Trash")
        item.tap()
        XCTAssertTrue(waitForVerb("untrash trash1").contains("untrash trash1"),
                      "restoring never reached the server")
    }

    /// The one action that asks: first a question, then the password.
    func testDeletingForGoodAsksTwice() {
        let app = launch(signedIn: true)
        inboxListed(app)
        open(app, list: "Trash")
        XCTAssertTrue(app.staticTexts["In the Trash: trash1"].waitForExistence(timeout: 10))
        let item = app.buttons["row.menu.deleteForever"]
        XCTAssertTrue(longPress(row(app, "In the Trash"), until: item), "no Delete forever")
        item.tap()
        let alert = app.alerts.firstMatch
        XCTAssertTrue(alert.waitForExistence(timeout: 5), "deleting for good did not ask")
        XCTAssertFalse(postedVerbs().contains("forever trash1"), "deleted before the answer")
        alert.buttons["Delete forever"].tap()
        confirmPassword(app)
        XCTAssertTrue(waitForVerb("forever trash1").contains("forever trash1"),
                      "deleting for good never reached the server")
    }

    /// Each list asks for itself: one that read the inbox would show
    /// the inbox's rows under its own title.
    func testPinnedSnoozedAndAllMailListTheirOwn() {
        let app = launch(signedIn: true)
        inboxListed(app)
        open(app, list: "Pinned")
        XCTAssertTrue(app.staticTexts["Pinned thread"].waitForExistence(timeout: 10), "Pinned")
        open(app, list: "Snoozed")
        XCTAssertTrue(app.staticTexts["Snoozed thread"].waitForExistence(timeout: 10), "Snoozed")
        open(app, list: "All mail")
        XCTAssertTrue(app.staticTexts["In All mail"].waitForExistence(timeout: 10), "All mail")
        XCTAssertFalse(app.staticTexts["Quarterly report and the follow-up notes"].exists,
                       "a list showed the inbox's rows")
    }

    func testReviewReleases() {
        let app = launch(signedIn: true)
        inboxListed(app)
        open(app, list: "Review")
        XCTAssertTrue(app.staticTexts["Your account will be suspended"].waitForExistence(timeout: 10),
                      "Review did not read the held conversations")
        let item = app.buttons["row.menu.release"]
        XCTAssertTrue(longPress(row(app, "Your account will be suspended"), until: item),
                      "no way out of Review")
        item.tap()
        XCTAssertTrue(waitForVerb("release held1").contains("release held1"),
                      "releasing never reached the server")
    }

    /// A label is a list of its own, and the row menu puts one on.
    func testALabelListsItsConversationsAndIsPutOn() {
        let app = launch(signedIn: true)
        inboxListed(app)
        let labels = app.buttons["row.menu.label"]
        XCTAssertTrue(longPress(row(app, "Quarterly report"), until: labels), "no Labels in the row menu")
        labels.tap()
        let work = app.buttons["row.menu.label.1"]
        XCTAssertTrue(work.waitForExistence(timeout: 5), "the reader's labels were not offered")
        work.tap()
        XCTAssertTrue(waitForVerb("label t1").contains("label t1"), "labelling never reached the server")

        app.buttons["Lists"].tap()
        let list = app.buttons["list.label.1"]
        XCTAssertTrue(list.waitForExistence(timeout: 5), "no label among the lists")
        list.tap()
        XCTAssertTrue(app.staticTexts["Labelled: lab1"].waitForExistence(timeout: 10),
                      "the label list did not ask for its label")
        XCTAssertTrue(app.navigationBars["Work"].waitForExistence(timeout: 5),
                      "the title is not the label's name")
    }
}
