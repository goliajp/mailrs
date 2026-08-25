import XCTest

/// The Mac app's own shell.
///
/// These are not the phone's tests run in a window: the phone's
/// screens are not in this target at all. What is asserted here is
/// what makes it a Mac app rather than an iOS app that happens to
/// launch — a source list, three columns at once, a real menu bar, and
/// the keyboard.
final class MacFlowTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        // The same stub the phone and iPad suites use, so this shows
        // real mail without an account. `mac-build.sh run` starts it;
        // the scheme's pre-action does too, so ⌘U from Xcode works.
        app.launchArguments = [
            "-mailrsBaseURL", "http://localhost:6039",
            "-mailrsToken", "stub-token",
            "-mailrs.language", "en",
            "-mailrsFreshCache",
        ]
        app.launch()
    }

    override func tearDown() {
        app.terminate()
    }

    /// The conversation rows, as things that can be clicked.
    ///
    /// The identifier propagates to the labels inside the row, so a
    /// bare prefix query matches those too — and one of them is not
    /// clickable. `.other` is what a SwiftUI list row is; a query
    /// naming `.cell` matched nothing and reported it as "no
    /// conversations".
    private func conversationRows() -> XCUIElementQuery {
        app.otherElements.matching(
            NSPredicate(format: "identifier BEGINSWITH 'row.conversation.'"))
    }

    /// The three columns, together, in one window — and the detail
    /// **saying** nothing is chosen rather than being blank, because a
    /// blank third of a window reads as something that failed to load.
    func testTheWindowHasThreeColumns() {
        XCTAssertTrue(
            app.staticTexts["Inbox"].waitForExistence(timeout: 30),
            "the source list never appeared")
        XCTAssertTrue(
            app.staticTexts["Mailboxes"].exists,
            "the source list had no section header, so it is not a source list")
        XCTAssertTrue(
            app.staticTexts["No conversation selected"].waitForExistence(timeout: 15),
            "the detail column was blank instead of saying nothing is chosen")
    }

    /// Choosing a conversation fills the detail **without the list
    /// going anywhere** — the property a pushed navigation cannot have,
    /// and the reason this is not the phone's design in a window.
    func testChoosingAConversationKeepsTheListOnScreen() {
        // Addressed by identifier. "The second outline" was a guess
        // about layout, and the sidebar is an outline too — the same
        // guess made an iPad test swipe the sidebar and report that
        // conversations offer no actions.
        let rows = conversationRows()
        XCTAssertTrue(
            rows.element(boundBy: 0).waitForExistence(timeout: 30),
            "no conversations in the middle column")
        rows.element(boundBy: 0).click()

        XCTAssertTrue(
            app.staticTexts["No conversation selected"].waitForNonExistence(timeout: 20),
            "the detail column still says nothing is chosen")
        // **Still there**, not "still the same number of them". A
        // dynamic query re-evaluates after the click and its count
        // moves with the selection's own elements — 17 to 16 here,
        // which the first version read as the list vanishing. What has
        // to be true is that the other two columns survived opening a
        // conversation, which a pushed navigation cannot do.
        XCTAssertTrue(
            rows.element(boundBy: 0).exists,
            "the conversation list went away when a conversation was opened")
        XCTAssertTrue(app.staticTexts["Inbox"].exists, "the source list went away")
    }

    /// **The menu bar.** An app whose only menu is Quit is the clearest
    /// sign of an iOS app in a window, and these two are the ones a
    /// mail client is expected to have.
    func testTheMenuBarOffersTheMailCommands() {
        // **By position, not by the word "File".** The system builds
        // the menu bar in the *system's* language, not the app's — this
        // machine's is Chinese, so the app launched in English had a
        // menu bar reading `Apple, MailrsMac, 文件, 编辑, …`. A test
        // that names the English word reports "no File menu" about a
        // menu bar that has one, and would keep reporting it on any
        // machine set to any other language.
        //
        // Index 2 is the File menu on every macOS: Apple, the app, then
        // File.
        let fileMenu = app.menuBars.menuBarItems.element(boundBy: 2)
        XCTAssertTrue(fileMenu.waitForExistence(timeout: 30), "the menu bar has no File menu")
        fileMenu.click()
        // The items themselves are the app's own strings, so these are
        // in the app's language and named as written.
        XCTAssertTrue(
            app.menuItems["New Message"].waitForExistence(timeout: 10),
            "File has no New Message")
        XCTAssertTrue(app.menuItems["Fetch Mail"].exists, "File has no Fetch Mail")
        // Closed again, so the next test does not start with a menu
        // hanging open over the window it is trying to read.
        app.typeKey(.escape, modifierFlags: [])
    }

    /// ⌘N opens the composer. The shortcut is declared on the menu
    /// command, so this is also the check that the command is wired to
    /// something rather than being a label.
    func testCommandNOpensTheComposer() {
        XCTAssertTrue(
            app.staticTexts["Inbox"].waitForExistence(timeout: 30), "the app never came up")
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(
            app.textFields["compose.to"].waitForExistence(timeout: 15)
                || app.textFields["composer-to"].waitForExistence(timeout: 5),
            "⌘N did not open the composer")
    }

    /// **⌘, opens Preferences.** A Mac app without a Settings scene
    /// has that shortcut greyed out and its options buried in the
    /// content, which is the phone's answer to a question this
    /// platform answers with a window.
    func testCommandCommaOpensPreferences() {
        XCTAssertTrue(
            app.staticTexts["Inbox"].waitForExistence(timeout: 30), "the app never came up")
        app.typeKey(",", modifierFlags: .command)
        XCTAssertTrue(
            app.staticTexts["Appearance"].waitForExistence(timeout: 15)
                || app.popUpButtons.firstMatch.waitForExistence(timeout: 5),
            "⌘, opened nothing")
        app.typeKey("w", modifierFlags: .command)
    }

    /// The toolbar carries the verbs for what is open, and they are
    /// **disabled until something is**. A button that looks available
    /// and does nothing is worse than one that says it cannot.
    func testTheToolbarActionsWaitForASelection() {
        XCTAssertTrue(
            app.staticTexts["No conversation selected"].waitForExistence(timeout: 30),
            "the app never came up")
        let archive = app.buttons["mac.toolbar.archive"]
        XCTAssertTrue(archive.waitForExistence(timeout: 10), "no Archive in the toolbar")
        XCTAssertFalse(
            archive.isEnabled,
            "Archive was offered with nothing selected")

        // By identifier, like the iPad's — "the second outline" was a
        // guess about layout, and a guess about layout is what made an
        // iPad test swipe the sidebar and report that conversations
        // offer no actions.
        let rows = conversationRows()
        XCTAssertTrue(
            rows.element(boundBy: 0).waitForExistence(timeout: 20), "no conversations")
        rows.element(boundBy: 0).click()
        XCTAssertTrue(
            waitUntil(timeout: 15) { archive.isEnabled },
            "Archive stayed disabled after a conversation was chosen")
    }

    private func waitUntil(timeout: TimeInterval, _ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            Thread.sleep(forTimeInterval: 0.25)
        }
        return condition()
    }
}
