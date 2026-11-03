import XCTest

extension MailrsUITestCase {
    /// Tap once the element has stopped moving.
    ///
    /// After `swipeUp` the scroll view keeps coasting, and a tap that
    /// lands while it coasts only stops the scroll: the button under the
    /// finger never sees it. `isHittable` does not tell the two apart —
    /// it says the element is on screen, not that it is still. Under
    /// load the coasting lasts longer, which is why a test that taps
    /// right after scrolling passes alone and fails in the full suite.
    ///
    /// Waits for the frame to read the same three times in a row, then
    /// taps once; it never taps twice, so a test that counts requests
    /// still counts one.
    func tapWhenSteady(
        _ element: XCUIElement, named: String,
        file: StaticString = #filePath, line: UInt = #line
    ) {
        var last = CGRect.null
        var same = 0
        for _ in 0..<40 {
            let now = element.frame
            if now == last {
                same += 1
            } else {
                same = 0
            }
            if same >= 2, element.isHittable { break }
            last = now
            Thread.sleep(forTimeInterval: 0.15)
        }
        XCTAssertTrue(element.isHittable, "\(named) is not reachable", file: file, line: line)
        element.tap()
    }
}
