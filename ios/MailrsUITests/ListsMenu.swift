import XCTest

extension MailrsUITestCase {
    /// Opens the Lists menu and taps `name` in it, scrolling the menu to
    /// it first: with every list the web has, the items at its end
    /// (Settings, Sign out) are below the fold on a phone, and a menu does
    /// not build the rows it has not shown.
    func tapInListsMenu(_ app: XCUIApplication, _ name: String,
                        file: StaticString = #filePath, line: UInt = #line) {
        let lists = app.buttons["Lists"]
        XCTAssertTrue(lists.waitForExistence(timeout: 15), "no Lists button", file: file, line: line)
        lists.tap()
        let item = app.buttons[name]
        if !item.waitForExistence(timeout: 2) {
            scrollTo(app, button: name, file: file, line: line)
        }
        item.tap()
    }
}
