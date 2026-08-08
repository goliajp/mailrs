import XCTest

/// Getting off a mailing list.
///
/// 42.6% of the mail in the real mailbox carries `List-Unsubscribe`,
/// and 91.7% of those accept RFC 8058 one-click, so this path is the
/// most-used one this client has that is not reading a message.
final class UnsubscribeFlowTests: MailrsUITestCase {

    /// The whole point, asserted at the wire: the client asks the server
    /// to leave the list, and the request names a **message**.
    ///
    /// A body carrying the URL would make the server a request
    /// forwarder aimed at anything a caller named, and would also mean
    /// the phone had the tracking URL in hand — which is the thing this
    /// design exists to avoid. The stub answers 400 to any body with a
    /// URL in it, so this fails rather than passes if that ever changes.
    func testOneClickUnsubscribeNamesTheMessageNotAUrl() {
        let app = launch(signedIn: true, folder: "Lists")

        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "systems design")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "no newsletter row")
        row.tap()

        let button = app.buttons["unsubscribe"].firstMatch
        // Existence is not reach: the footer sits under a rendered
        // body, and a tap on an element below the fold lands somewhere
        // else entirely.
        scrollTo(app, element: button, named: "unsubscribe")
        XCTAssertTrue(button.isHittable, "the unsubscribe button is not reachable")
        button.tap()

        XCTAssertTrue(
            app.staticTexts["unsubscribed"].waitForExistence(timeout: 10),
            "the button never resolved to a result"
        )

        let requests = unsubscribeRequests()
        // `guard` rather than a subscript: an empty array here is the
        // failure this test is for, and indexing it crashes the runner
        // before it can say so.
        guard let first = requests.first else {
            XCTFail("no unsubscribe reached the server")
            return
        }
        XCTAssertEqual(requests.count, 1, "expected exactly one: \(requests)")
        XCTAssertEqual(first["thread_id"] as? String, "t3")
        XCTAssertEqual(first["uid"] as? Int, 8)
        XCTAssertNil(first["url"], "the client must not send a URL")
    }

    /// A refusal is said out loud, and the sender's own link stays.
    ///
    /// An unsubscribe that fails silently while looking like it worked
    /// is how someone ends up tapping the same button every week for a
    /// year.
    func testARefusedUnsubscribeSaysSo() {
        let app = launch(signedIn: true, folder: "Lists")
        refuseUnsubscribe()

        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "systems design")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "no newsletter row")
        row.tap()

        let button = app.buttons["unsubscribe"].firstMatch
        scrollTo(app, element: button, named: "unsubscribe")
        button.tap()

        XCTAssertTrue(
            app.staticTexts["unsubscribe-failed"].waitForExistence(timeout: 10),
            "a refusal was not reported"
        )
        XCTAssertTrue(app.buttons["unsubscribe"].exists, "the way to try again vanished")
    }

    /// Tell the stub to refuse the next unsubscribe. `POST` because the
    /// stub's other switches are POSTs, and a GET that changes state is
    /// a trap for whoever reads the log.
    private func refuseUnsubscribe() {
        guard let url = URL(string: "http://localhost:6039/debug/unsubscribe-refuse") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let done = expectation(description: "refuse")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }
}
