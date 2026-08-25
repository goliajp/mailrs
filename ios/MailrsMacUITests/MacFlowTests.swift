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
        // **The second outline.** Both the source list and the
        // conversation list are outlines, so `app.outlines.cells` is
        // the two of them mixed — eleven rows, the first of which is
        // the Inbox row in the sidebar. Clicking that selected a
        // mailbox and left the detail exactly as it was, which the
        // assertion then reported as "nothing is chosen".
        XCTAssertTrue(
            app.outlines.element(boundBy: 1).waitForExistence(timeout: 30),
            "there is no second column — the window is not split")
        let rows = app.outlines.element(boundBy: 1).cells
        XCTAssertTrue(
            rows.element(boundBy: 0).waitForExistence(timeout: 20),
            "no conversations in the middle column")
        let before = rows.count
        rows.element(boundBy: 0).click()

        XCTAssertTrue(
            app.staticTexts["No conversation selected"].waitForNonExistence(timeout: 20),
            "the detail column still says nothing is chosen. rows: \(rows.count), "
                + "outlines: \(app.outlines.count), tables: \(app.tables.count)")
        XCTAssertGreaterThanOrEqual(
            rows.count, before,
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
}
