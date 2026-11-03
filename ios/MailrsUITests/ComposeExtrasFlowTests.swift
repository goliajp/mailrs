import XCTest

/// What the composer gained to match the web: the From choice, undoing a
/// send, the formatting row, templates, a resumed draft keeping its time
/// and address — and a tapped notification opening its conversation.
final class ComposeExtrasFlowTests: MailrsUITestCase {

    func testSendsFromTheChosenAddress() {
        let app = launch(signedIn: true)
        setSendAs(["sales@golia.jp"])
        openComposer(app, to: "someone@example.com")

        let from = element(app, "composer-from")
        XCTAssertTrue(from.waitForExistence(timeout: 10), "no From choice with a send-as address")
        from.tap()
        let sales = app.buttons["sales@golia.jp"]
        XCTAssertTrue(sales.waitForExistence(timeout: 5), "the send-as address was not offered")
        sales.tap()

        app.textFields["composer-subject"].tap()
        app.textFields["composer-subject"].typeText("From sales")
        app.buttons["Send"].tap()

        XCTAssertTrue(element(app, "undo-send").waitForExistence(timeout: 10), "the send never finished")
        XCTAssertEqual(sentMessages().last?["from"] as? String, "sales@golia.jp",
                       "the message did not leave by the chosen address")
    }

    func testUndoStopsAHeldSend() {
        let app = launch(signedIn: true)
        openComposer(app, to: "someone@example.com")
        app.textFields["composer-subject"].tap()
        app.textFields["composer-subject"].typeText("Oops")
        app.buttons["Send"].tap()

        let undo = element(app, "undo-send")
        XCTAssertTrue(undo.waitForExistence(timeout: 10), "no Undo after sending")
        XCTAssertEqual(sentMessages().last?["undo_seconds"] as? Int, 10,
                       "the send did not ask the server to hold it")
        XCTAssertNil(sentMessages().last?["html_body"], "a send with no formatting carried html_body")
        undo.tap()

        let status = app.staticTexts["send-undo-status"]
        XCTAssertTrue(status.waitForExistence(timeout: 5), "the bar said nothing after Undo")
        XCTAssertTrue(waitForLabel(status, "Send cancelled"), "Undo did not report the send cancelled")
        XCTAssertEqual(pendingCancels(), ["<stub-reply@golia.jp>"],
                       "Undo did not ask the server to stop this send")
    }

    func testUndoTooLateSaysTheMessageLeft() {
        let app = launch(signedIn: true)
        makeUndoTooLate()
        openComposer(app, to: "someone@example.com")
        app.buttons["Send"].tap()

        let undo = element(app, "undo-send")
        XCTAssertTrue(undo.waitForExistence(timeout: 10), "no Undo after sending")
        undo.tap()

        let status = app.staticTexts["send-undo-status"]
        XCTAssertTrue(status.waitForExistence(timeout: 5), "the bar said nothing after Undo")
        XCTAssertTrue(waitForLabel(status, "Too late to undo — already sent"),
                      "a refused Undo was reported as working")
    }

    func testTheBoldButtonWritesMarkdown() {
        let app = launch(signedIn: true)
        openComposer(app, to: "someone@example.com")
        let body = app.textViews.firstMatch
        body.tap()
        body.typeText("Hello ")
        element(app, "format-bold").tap()
        body.typeText("big")
        app.buttons["Send"].tap()

        XCTAssertTrue(element(app, "undo-send").waitForExistence(timeout: 10), "the send never finished")
        let last = sentMessages().last
        let sent = last?["body"] as? String ?? ""
        XCTAssertTrue(sent.hasPrefix("Hello **big**"), "bold did not write Markdown marks: \(sent)")
        let html = last?["html_body"] as? String ?? ""
        XCTAssertTrue(html.hasPrefix("<!DOCTYPE html>"), "the formatted send carried no html_body")
        XCTAssertTrue(html.contains("<p>Hello <strong>big</strong>"), "html_body is not the rendered markdown: \(html)")
    }

    func testATemplateFillsSubjectAndBody() {
        let app = launch(signedIn: true)
        openComposer(app, to: "someone@example.com")
        let menu = element(app, "composer-templates")
        XCTAssertTrue(menu.waitForExistence(timeout: 10), "no template menu")
        menu.tap()
        let thanks = app.buttons["Thanks"]
        XCTAssertTrue(thanks.waitForExistence(timeout: 5), "the template was not listed")
        thanks.tap()

        XCTAssertEqual(app.textFields["composer-subject"].value as? String, "Thank you",
                       "the template's subject was not applied")
        app.buttons["Send"].tap()
        XCTAssertTrue(element(app, "undo-send").waitForExistence(timeout: 10), "the send never finished")
        let body = sentMessages().last?["body"] as? String ?? ""
        XCTAssertTrue(body.hasPrefix("Thank you for your message."), "the template's body was not applied: \(body)")
    }

    func testAResumedDraftKeepsItsTimeAndAddress() {
        let app = launch(signedIn: true)
        setSendAs(["sales@golia.jp"])
        let when = Int(Date().timeIntervalSince1970) + 86_400
        storeDraft([
            "to": "later@example.com", "subject": "Timed", "body": "Later",
            "from": "sales@golia.jp", "scheduled_at": when,
        ])
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        app.buttons["Lists"].tap()
        app.buttons["Drafts"].tap()
        XCTAssertTrue(app.staticTexts["Timed"].waitForExistence(timeout: 10), "the draft was not listed")
        app.staticTexts["Timed"].tap()

        XCTAssertTrue(element(app, "composer-scheduled").waitForExistence(timeout: 5),
                      "the resumed draft lost its time")
        app.buttons["Send"].tap()

        XCTAssertTrue(waitForSend(), "the send never reached the stub")
        let sent = sentMessages().last
        XCTAssertEqual(sent?["scheduled_at"] as? Int, when, "Send dropped the draft's time")
        XCTAssertEqual(sent?["from"] as? String, "sales@golia.jp", "Send dropped the draft's address")
    }

    func testANotificationTapOpensItsConversation() {
        let app = launch(signedIn: true, extraArguments: ["-mailrsNotificationTap", "t1"])
        let subject = app.staticTexts["thread-subject"]
        XCTAssertTrue(subject.waitForExistence(timeout: 15), "the notified conversation never opened")
        XCTAssertEqual(subject.label, "Quarterly report and the follow-up notes")
    }

    private func waitForLabel(_ element: XCUIElement, _ text: String) -> Bool {
        let matched = NSPredicate(format: "label == %@", text)
        return XCTWaiter.wait(for: [expectation(for: matched, evaluatedWith: element)], timeout: 8) == .completed
    }

    private func waitForSend() -> Bool {
        for _ in 0..<20 {
            if !sentMessages().isEmpty { return true }
            Thread.sleep(forTimeInterval: 0.5)
        }
        return false
    }
}
