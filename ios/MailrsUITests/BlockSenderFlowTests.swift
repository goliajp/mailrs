import XCTest

/// Blocking a sender from their message: the menu names the address, the
/// question says where the mail goes, the block and the conversation's move
/// reach the server as one call, and the undo bar takes it back. The same
/// journey as Android's `BlockSenderFlowTest`.
final class BlockSenderFlowTests: MailrsUITestCase {

    func testASenderIsBlockedFromTheirMessageAndTheUndoTakesItBack() {
        let app = launch(signedIn: true)
        openAlicesMessage(app)
        let sender = app.staticTexts["Alice Smith"].firstMatch
        XCTAssertTrue(sender.waitForExistence(timeout: 10), "the older message's sender was not shown")
        sender.press(forDuration: 1.0)

        let block = element(app, "sender.block")
        XCTAssertTrue(block.waitForExistence(timeout: 10), "the sender menu offered no Block")
        XCTAssertTrue(block.label.contains("alice@example.com"), "Block did not name the address: \(block.label)")
        block.tap()

        let question = app.alerts.firstMatch
        XCTAssertTrue(question.waitForExistence(timeout: 5), "blocking did not ask first")
        XCTAssertFalse(postedVerbs().contains("block-sender t1"), "blocked before the answer")
        question.buttons["Block"].firstMatch.tap()

        XCTAssertTrue(waitForVerb("block-sender t1"), "the block never reached the server")
        let undo = app.buttons["undo-archive"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5), "no undo offered")
        undo.tap()
        XCTAssertTrue(waitForVerb("unblock-sender t1"), "the undo never reached the server")

        // unblocked again: the menu offers Block, not Unblock
        openAlicesMessage(app)
        let again = app.staticTexts["Alice Smith"].firstMatch
        XCTAssertTrue(again.waitForExistence(timeout: 10))
        again.press(forDuration: 1.0)
        XCTAssertTrue(element(app, "sender.block").waitForExistence(timeout: 10), "the undo left the sender blocked")
    }

    private func waitForVerb(_ verb: String) -> Bool {
        for _ in 0..<20 {
            if postedVerbs().contains(verb) { return true }
            Thread.sleep(forTimeInterval: 0.25)
        }
        return false
    }

    /// Alice's message in the Quarterly report, unfolded.
    private func openAlicesMessage(_ app: XCUIApplication) {
        let row = app.buttons.containing(NSPredicate(format: "label CONTAINS %@", "Quarterly report")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        let folded = app.buttons["collapsed-1"]
        XCTAssertTrue(folded.waitForExistence(timeout: 10), "the older message was not there to unfold")
        folded.tap()
    }
}
