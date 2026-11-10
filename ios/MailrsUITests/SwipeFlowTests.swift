import XCTest

/// A full swipe on a row does what the reader's swipe setting says, the
/// setting the web and Android read too: right archives and left moves to
/// the Trash until it is changed, and both offer the undo. The same
/// journeys as Android's `SwipeFlowTest`.
final class SwipeFlowTests: MailrsUITestCase {

    private func quarterlyReport(_ app: XCUIApplication) -> XCUIElement {
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        return row
    }

    /// Drag a finger across the whole row, past the point where the first
    /// action takes the swipe. The short swipe that reveals the buttons is
    /// `swipeAndTap`'s; this is the gesture that does it in one movement.
    private func fullSwipe(_ row: XCUIElement, right: Bool) {
        var from = 0.92
        var to = 0.0
        if right {
            from = 0.08
            to = 1.0
        }
        let start = row.coordinate(withNormalizedOffset: CGVector(dx: from, dy: 0.5))
        let end = row.coordinate(withNormalizedOffset: CGVector(dx: to, dy: 0.5))
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .fast, thenHoldForDuration: 0.1)
    }

    /// The default right swipe was "mark read" until the setting existed;
    /// a row that stays listed after it is that old behaviour.
    func testASwipeRightArchivesAndTheUndoBringsItBack() {
        let app = launch(signedIn: true)
        let row = quarterlyReport(app)
        fullSwipe(row, right: true)
        XCTAssertTrue(row.waitForNonExistence(timeout: 10), "a right swipe did not take the row away")
        let undo = app.buttons["undo-archive"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5), "no undo offered")
        XCTAssertTrue(postedVerbs().contains("archive t1"), "the swipe was not an archive: \(postedVerbs())")
        undo.tap()
        XCTAssertTrue(row.waitForExistence(timeout: 10), "the undo did not bring it back")
        XCTAssertTrue(recordedWrites().contains { $0.hasSuffix("/t1/unarchive") }, "the undo never unarchived on the wire")
    }

    func testASwipeLeftMovesToTheTrashAndTheUndoRestores() {
        let app = launch(signedIn: true)
        let row = quarterlyReport(app)
        fullSwipe(row, right: false)
        XCTAssertTrue(row.waitForNonExistence(timeout: 10), "a left swipe did not take the row away")
        XCTAssertFalse(app.staticTexts["This will permanently delete all messages."].exists,
                       "the swipe asked as though it deleted for good")
        let undo = app.buttons["undo-archive"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5), "no undo offered")
        XCTAssertTrue(postedVerbs().contains("delete t1"), "the swipe did not move it to the Trash: \(postedVerbs())")
        undo.tap()
        XCTAssertTrue(row.waitForExistence(timeout: 10), "the undo did not bring it back")
        XCTAssertTrue(postedVerbs().contains("untrash t1"), "the undo never restored on the wire: \(postedVerbs())")
    }

    /// Choosing "mark read" for the right swipe is saved on the server and
    /// the next right swipe reads the row in place instead of archiving it.
    func testTheSettingChangesWhatASwipeDoes() {
        let app = launch(signedIn: true)
        tapInListsMenu(app, "open-settings")
        let picker = element(app, "settings.swipe.right")
        scrollTo(app, element: picker, named: "settings.swipe.right")
        picker.tap()
        app.buttons["Mark read or unread"].tap()
        XCTAssertTrue(savedSwipeRight(is: "read"), "the choice did not reach the server")
        app.navigationBars.buttons["Done"].tap()
        if app.buttons["Inbox"].waitForExistence(timeout: 3) { app.buttons["Inbox"].tap() }

        let unread = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@ AND label CONTAINS %@", "Quarterly report", "Unread")
        ).firstMatch
        XCTAssertTrue(unread.waitForExistence(timeout: 15), "no unread row to swipe")
        fullSwipe(unread, right: true)
        XCTAssertTrue(unread.waitForNonExistence(timeout: 10), "the row is still announced as unread")
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"].exists,
                      "the swipe took the row away instead of reading it")
        XCTAssertFalse(postedVerbs().contains("archive t1"), "the swipe still archived")
    }

    /// Whether the stub's last swipe save set the right swipe to `choice`.
    private func savedSwipeRight(is choice: String) -> Bool {
        for _ in 0..<20 {
            if lastSwipeSave()?["right"] as? String == choice { return true }
            Thread.sleep(forTimeInterval: 0.25)
        }
        return false
    }

    private func lastSwipeSave() -> [String: Any]? {
        guard let url = URL(string: "http://localhost:6039/debug/settings") else { return nil }
        var found: [String: Any]?
        let data = stubData(url)
        if let data, let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
            found = list.last { ($0["path"] as? String) == "/api/settings/swipe" }?["body"] as? [String: Any]
        }
        return found
    }
}
