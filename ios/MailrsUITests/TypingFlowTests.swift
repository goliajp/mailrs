import XCTest

/// Every character typed into a message body arrives, however fast it
/// is typed and whatever the composer is loading meanwhile.
final class TypingFlowTests: MailrsUITestCase {
    private static let line = "The quick brown fox jumps over the lazy dog, 0123456789. "

    func testFastTypingInTheComposerKeepsEveryCharacter() {
        resetStub()
        let app = launch(signedIn: true)
        openComposer(app, to: "someone@example.com")
        let body = app.textViews.firstMatch
        body.tap()
        // typed straight after the composer opens, while its first loads
        // are still landing
        let typed = String(repeating: Self.line, count: 3)
        body.typeText(typed)
        app.buttons["Send"].tap()

        XCTAssertTrue(element(app, "undo-send").waitForExistence(timeout: 10), "the send never finished")
        // the signature follows what was typed, and trailing space is trimmed
        let sent = sentMessages().last?["body"] as? String ?? ""
        let expected = typed.trimmingCharacters(in: .whitespaces)
        XCTAssertTrue(sent.hasPrefix(expected), "characters were lost or moved while typing: \(sent)")
    }

    func testFastTypingInAReplyKeepsEveryCharacter() {
        resetStub()
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        app.buttons["Reply"].tap()
        let editor = app.textViews.firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 5), "no composer")
        editor.tap()
        let typed = String(repeating: Self.line, count: 3)
        editor.typeText(typed)
        let value = editor.value as? String ?? ""
        XCTAssertTrue(value.hasPrefix(typed), "characters were lost or moved while typing: \(value)")
    }
}
