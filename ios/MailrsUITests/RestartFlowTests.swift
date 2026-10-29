import XCTest

/// The server restarting during a deploy answers 503 for a few seconds. A
/// read waits as told and goes again once; a write is never sent twice and
/// says the server is restarting instead.
final class RestartFlowTests: MailrsUITestCase {
    private func restarting(_ path: String) {
        guard let url = URL(string: "http://localhost:6039/debug/restarting/1?path=\(path)") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let done = expectation(description: "restarting")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }

    /// Answers the stub gave the armed path after refusing it: the read
    /// coming back, which the screen cannot show (it may draw from a cache).
    private func servedAfter() -> Int {
        guard let url = URL(string: "http://localhost:6039/debug/restart-state") else { return -1 }
        var result = -1
        let done = expectation(description: "restart-state")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                result = json["served_after"] as? Int ?? -1
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }

    func testAConversationOpenedDuringARestartStillOpens() {
        let app = launch(signedIn: true)
        let subject = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(subject.waitForExistence(timeout: 15), "inbox never listed")
        restarting("/api/conversations/t1")
        subject.tap()
        let came = NSPredicate { _, _ in self.servedAfter() >= 1 }
        let back = expectation(for: came, evaluatedWith: nil)
        wait(for: [back], timeout: 8)
        XCTAssertFalse(
            app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "restarting"))
                .firstMatch.exists,
            "a read was told the server is restarting")
    }

    func testAWriteDuringARestartIsSaidAndNotRepeated() {
        let app = launch(signedIn: true)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15), "inbox never listed")
        restarting("/api/conversations/batch")
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        swipeAndTap(app, row: row, edge: .trailing, action: "Archive")

        XCTAssertTrue(
            app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "restarting"))
                .firstMatch.waitForExistence(timeout: 10),
            "the refusal was not said")
        // a second send would have reached the stub, which is no longer
        // restarting, and been recorded; any repeat happens before the
        // answer is shown
        XCTAssertFalse(
            recordedWrites().contains { $0.contains("/api/conversations/batch") },
            "the write was sent again: \(recordedWrites())")
    }
}
