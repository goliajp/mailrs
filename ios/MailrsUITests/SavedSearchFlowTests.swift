import XCTest

/// The empty search field offers the searches saved on the web and the
/// operators the server takes; choosing a saved search runs its query.
/// The same journey as Android's `SavedSearchFlowTest`.
final class SavedSearchFlowTests: MailrsUITestCase {

    func testASavedSearchRunsItsQuery() {
        let app = launch(signedIn: true)
        let english = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(english.waitForExistence(timeout: 15), "inbox never listed")

        let field = app.searchFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5), "no search field")
        field.tap()

        let saved = element(app, "search.saved.ss_ref")
        XCTAssertTrue(saved.waitForExistence(timeout: 10), "the empty search offered no saved search")
        XCTAssertTrue(element(app, "search.saved.ss_ops").exists, "a saved search with operators was left out")
        XCTAssertTrue(element(app, "search.operators").exists, "the search operators were not named")

        saved.tap()

        XCTAssertEqual(field.value as? String, "請求", "the field does not hold the saved query")
        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].waitForExistence(timeout: 10),
                      "the saved search's match was not shown")
        XCTAssertTrue(english.waitForNonExistence(timeout: 5),
                      "a thread the saved query does not match stayed on screen")
    }
}
