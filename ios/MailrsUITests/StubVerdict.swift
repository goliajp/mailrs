import XCTest

extension MailrsUITestCase {
    /// Arm the suspicious case on the ordinary thread (`stub_verdict.py`):
    /// a held verdict, sender checks, a flagged link and a callback number
    /// on its second message. `launch` resets the stub, so arm after it.
    func armVerdicts() {
        guard let url = URL(string: "http://localhost:6039/debug/verdict") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let done = expectation(description: "debug/verdict")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }

    /// Open the ordinary thread from the inbox.
    func openOrdinaryThread(_ app: XCUIApplication) {
        let row = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
    }

    /// Drag the thread up until `element` sits between the navigation bar
    /// and the toolbar. Existence is not enough here: a link in a message
    /// body exists while it is below the fold, and a tap there lands on
    /// the toolbar.
    func bringIntoView(_ app: XCUIApplication, _ element: XCUIElement, named: String) {
        let screen = app.frame
        for _ in 0..<10 {
            let f = element.frame
            if element.exists, f.minY > screen.minY + 130, f.maxY < screen.maxY - 120 { return }
            let from = app.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.7))
            from.press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.45)))
        }
        XCTFail("never brought \(named) into view: \(element.frame)")
    }

    /// Any element by its accessibility identifier, whatever it renders as.
    func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any)[id].firstMatch
    }
}
