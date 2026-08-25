import XCTest

/// The iPad's own layout.
///
/// Everything else in this bundle runs on the phone simulator, where
/// `PadLayout.splits` is false and these screens do not exist at all —
/// so this file is reached only through `./scripts/ios-build.sh ipad`,
/// and a run on the phone would report it green by never reaching it.
///
/// The witness in each test is that **the columns are on screen at
/// once**. That is the whole difference from the phone, and a test
/// that only checked "a conversation opened" would pass on the phone
/// layout stretched.
final class PadFlowTests: MailrsUITestCase {
    /// The conversation rows, as things that can be tapped.
    ///
    /// The identifier propagates from the row to the text inside it,
    /// so a bare prefix query matches the row **and** its labels and
    /// its avatar — the avatar is not hittable, and tapping it fails
    /// with "Failed to not hittable" rather than anything about
    /// conversations.
    ///
    /// `.other` is what a SwiftUI `List` row is here — measured, not
    /// assumed: a query that named `.cell` matched nothing at all, and
    /// reported it as "no conversations".
    private func conversationRows(_ app: XCUIApplication) -> XCUIElementQuery {
        app.otherElements.matching(
            NSPredicate(format: "identifier BEGINSWITH 'row.conversation.'"))
    }

    /// A sidebar row, by identifier and **not by element type**: a
    /// `List` row is a cell in one place and a button in another, and
    /// a test that names the type reports "there is no Archived
    /// mailbox" when what it means is "I looked in the wrong kind of
    /// element".
    private func sidebarRow(_ app: XCUIApplication, _ list: String) -> XCUIElement {
        app.descendants(matching: .any)["pad.list.\(list)"].firstMatch
    }

    /// Sidebar, list and detail, together — and the empty detail
    /// **saying so** rather than being blank, because a blank pane
    /// with no explanation reads as something that failed to load.
    func testTheThreeColumnsAreOnScreenAtOnce() {
        let app = launch(signedIn: true)
        XCTAssertTrue(
            sidebarRow(app, "inbox").waitForExistence(timeout: 20),
            "the sidebar never appeared — is this running on the phone simulator?")
        XCTAssertTrue(
            app.cells.element(boundBy: 0).waitForExistence(timeout: 15),
            "the conversation column was empty")
        XCTAssertTrue(
            app.staticTexts["No conversation selected"].waitForExistence(timeout: 10),
            "the detail column was blank instead of saying nothing is chosen")
    }

    /// Choosing a conversation fills the detail **without leaving the
    /// list** — the property the phone cannot have.
    func testChoosingAConversationKeepsTheListOnScreen() {
        let app = launch(signedIn: true)
        let rows = app.cells
        XCTAssertTrue(rows.element(boundBy: 0).waitForExistence(timeout: 20), "no conversations")
        let before = rows.count
        rows.element(boundBy: 0).tap()

        XCTAssertTrue(
            app.staticTexts["No conversation selected"].waitForNonExistence(timeout: 15),
            "the detail column still says nothing is chosen")
        // **The list is still there.** On a pushed navigation it would
        // not be, and that is exactly what this layout exists to avoid.
        XCTAssertGreaterThanOrEqual(
            rows.count, before,
            "the conversation list went away when a conversation was opened")
    }

    /// The sidebar switches mailbox without disturbing the other two
    /// columns.
    ///
    /// Tapped by its visible text rather than by identifier: the
    /// identifier resolves for the row that is selected on launch and
    /// not reliably for the others, and a query that finds one row and
    /// not its neighbour reports "there is no Archived mailbox" about
    /// a sidebar that plainly has one. Tests launch in English, so the
    /// label is stable.
    func testTheSidebarSwitchesMailbox() {
        let app = launch(signedIn: true)
        let archived = app.staticTexts["Archived"]
        XCTAssertTrue(archived.waitForExistence(timeout: 20), "no Archived row in the sidebar")
        archived.tap()
        XCTAssertTrue(
            app.navigationBars["Archived"].waitForExistence(timeout: 10),
            "the list column did not follow the sidebar")
        // Still three columns: switching mailbox must not collapse the
        // layout back to one.
        XCTAssertTrue(
            sidebarRow(app, "inbox").exists,
            "the sidebar went away when a mailbox was chosen")
    }

    /// **The swipes are back.** Converting the middle column from
    /// `NavigationLink` rows to a selection dropped them, and losing
    /// archive-by-swipe is losing the gesture triage is actually done
    /// with — on the device with the most room to do it.
    func testAConversationCanBeArchivedBySwiping() {
        let app = launch(signedIn: true)
        // **By identifier**, not by "the first cell on screen": the
        // sidebar is a list too, and `app.cells` is the two of them
        // mixed. Addressing the sidebar's first row and swiping it
        // reported that swiping a conversation offers nothing — true
        // of the row it reached, and not of the row it meant.
        let rows = conversationRows(app)
        XCTAssertTrue(
            rows.element(boundBy: 0).waitForExistence(timeout: 20), "no conversations")
        let before = rows.count
        rows.element(boundBy: 0).swipeLeft()
        XCTAssertTrue(
            app.buttons["Archive"].waitForExistence(timeout: 5),
            "swiping a conversation offered nothing")
        app.buttons["Archive"].tap()
        XCTAssertTrue(
            waitUntil(timeout: 15) { rows.count < before },
            "the row is still there after archiving it")
    }

    /// The open mailbox carries its unread count and the others carry
    /// nothing — the server gives one total, and printing it beside
    /// every row would say "four unread in Archived".
    func testOnlyTheOpenMailboxCarriesACount() {
        let app = launch(signedIn: true)
        XCTAssertTrue(
            sidebarRow(app, "inbox").waitForExistence(timeout: 20), "no sidebar")
        // The stub's inbox has unread rows, so Inbox carries a badge
        // and Archived does not. Read through the row's label, which is
        // where a badge lands for VoiceOver.
        let archived = sidebarRow(app, "archived")
        XCTAssertFalse(
            archived.label.contains("2") || archived.label.contains("3"),
            "a mailbox that was never counted is showing a number: \(archived.label)")
    }

    private func waitUntil(timeout: TimeInterval, _ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            Thread.sleep(forTimeInterval: 0.25)
        }
        return condition()
    }

    /// **Switching mailbox must not leave the last message on screen.**
    ///
    /// A defect only this layout can have: open something from Inbox,
    /// switch to Archived, and the detail column went on showing a
    /// conversation that is not in the list beside it and cannot be
    /// reached from it. The phone cannot do this — it pushes, so
    /// leaving the list means leaving the message.
    func testSwitchingMailboxClearsWhatWasOpen() {
        let app = launch(signedIn: true)
        let rows = conversationRows(app)
        XCTAssertTrue(
            rows.element(boundBy: 0).waitForExistence(timeout: 20),
            "no conversations")
        rows.element(boundBy: 0).tap()
        XCTAssertTrue(
            app.staticTexts["No conversation selected"].waitForNonExistence(timeout: 15),
            "the conversation never opened")

        app.staticTexts["Archived"].tap()
        XCTAssertTrue(
            app.staticTexts["No conversation selected"].waitForExistence(timeout: 15),
            "the detail column is still showing mail from the mailbox that was left")
    }

    /// ⌘R and pull-to-refresh both fetch. The phone has had the
    /// gesture from the beginning; the iPad had neither, so the only
    /// way to fetch was a button somebody had to find.
    func testTheListCanBeRefreshed() {
        let app = launch(signedIn: true)
        XCTAssertTrue(
            app.buttons["pad.sync"].waitForExistence(timeout: 20),
            "there is no way to fetch mail on this screen")
        app.buttons["pad.sync"].tap()
        XCTAssertTrue(
            conversationRows(app).element(boundBy: 0).waitForExistence(timeout: 20),
            "fetching emptied the list")
    }

    /// **Archiving by swipe offers a way back.**
    ///
    /// The gesture arrived on this screen before the retraction did,
    /// which is the worse half to have: one swipe and the conversation
    /// is gone with nothing to press. The phone has had the undo bar
    /// from the beginning.
    /// A refused archive says so.
    ///
    /// `session.banner` is written on every failure and, until this
    /// was added, **nothing on this screen read it** — the row went
    /// back to where it was and the reader was left to decide whether
    /// they had imagined pressing the button.
    func testARefusedArchiveSaysSo() {
        let app = launch(signedIn: true)
        // After the launch: launching resets the stub.
        refuseVerb("archive")
        let rows = conversationRows(app)
        XCTAssertTrue(
            rows.element(boundBy: 0).waitForExistence(timeout: 20), "no conversations")
        rows.element(boundBy: 0).swipeLeft()
        XCTAssertTrue(
            app.buttons["Archive"].waitForExistence(timeout: 5),
            "swiping a conversation offered nothing")
        app.buttons["Archive"].tap()
        XCTAssertTrue(
            app.descendants(matching: .any)["error-banner"].waitForExistence(timeout: 10),
            "the refusal was silent")
    }

    func testArchivingOffersUndo() {
        let app = launch(signedIn: true)
        let rows = conversationRows(app)
        XCTAssertTrue(rows.element(boundBy: 0).waitForExistence(timeout: 20), "no conversations")
        // **Followed by what it says**, not by the element that holds
        // it. Undo reinserts the row and SwiftUI rebuilds it, so the
        // container is a different element with the same identifier —
        // a query on the container reports the message gone when it is
        // back on screen. The phone's own undo test follows the
        // subject for the same reason.
        let subject = "Quarterly report"
        let message = app.staticTexts.containing(
            NSPredicate(format: "label CONTAINS %@", subject)
        ).firstMatch
        XCTAssertTrue(message.waitForExistence(timeout: 20), "that conversation is not listed")

        rows.element(boundBy: 0).swipeLeft()
        XCTAssertTrue(app.buttons["Archive"].waitForExistence(timeout: 5), "no Archive")
        app.buttons["Archive"].tap()
        XCTAssertTrue(
            message.waitForNonExistence(timeout: 15), "the row survived being archived")

        let undo = app.buttons["undo-archive"]
        XCTAssertTrue(
            undo.waitForExistence(timeout: 10),
            "a conversation was archived and nothing offered to undo it")
        undo.tap()
        XCTAssertTrue(
            message.waitForExistence(timeout: 15),
            "Undo was pressed and the conversation did not come back")
    }
}
