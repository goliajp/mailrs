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

    /// A refused archive says so.
    ///
    /// `session.banner` is written on every failure and, until this
    /// was added, **nothing in this window read it** — the toolbar
    /// button came back up and the reader was left to decide whether
    /// the mail had moved.
    func testARefusedArchiveSaysSo() {
        let rows = conversationRows()
        XCTAssertTrue(
            rows.element(boundBy: 0).waitForExistence(timeout: 30), "no conversations")
        // After the app is up: the stub is reset on launch, so a
        // refusal armed first is wiped by the very next call.
        refuseVerb("archive")
        rows.element(boundBy: 0).click()
        let archive = app.buttons["mac.toolbar.archive"]
        XCTAssertTrue(
            waitUntil(timeout: 15) { archive.isEnabled },
            "Archive stayed disabled after a conversation was chosen")
        archive.click()
        XCTAssertTrue(
            app.descendants(matching: .any)["error-banner"].waitForExistence(timeout: 10),
            "the refusal was silent")
    }

    /// Starring and moving are in the toolbar, and reach the server.
    ///
    /// Both verbs existed in `Session` and were on nothing in this
    /// window. A Mac has no swipes to put them on, so the toolbar is
    /// where they go — and a menu that is only reachable by
    /// right-clicking is a menu most people never find.
    func testStarringAndMovingReachTheServer() {
        let rows = conversationRows()
        XCTAssertTrue(
            rows.element(boundBy: 0).waitForExistence(timeout: 30), "no conversations")
        rows.element(boundBy: 0).click()

        let star = app.buttons["mac.toolbar.star"]
        XCTAssertTrue(star.waitForExistence(timeout: 10), "no Star in the toolbar")
        XCTAssertTrue(
            waitUntil(timeout: 15) { star.isEnabled },
            "Star stayed disabled after a conversation was chosen")
        star.click()
        XCTAssertTrue(
            waitUntil(timeout: 15) {
                postedVerbs().contains { $0.hasPrefix("star ") }
            },
            "starring never reached the server: \(postedVerbs())")

        let move = app.descendants(matching: .any)["mac.toolbar.move"].firstMatch
        XCTAssertTrue(move.waitForExistence(timeout: 10), "no Move in the toolbar")
        move.click()
        // `menuItems`, not `buttons`: a SwiftUI `Menu` in a Mac
        // toolbar opens an NSMenu, and a query naming the wrong type
        // reports "Move offered no destinations" when what it means is
        // "I looked in the wrong kind of element".
        let promotion = app.menuItems["Mark as promotion"].firstMatch
        XCTAssertTrue(
            promotion.waitForExistence(timeout: 5),
            "Move opened no menu: \(app.debugDescription.prefix(2000))")
        promotion.click()
        XCTAssertTrue(
            waitUntil(timeout: 15) {
                postedVerbs().contains { $0.hasPrefix("mark-promotion ") }
            },
            "the move never reached the server: \(postedVerbs())")
    }

    /// Every verb the stub has been sent since it was reset.
    private func postedVerbs() -> [String] {
        guard let url = URL(string: "http://localhost:6039/debug/verbs") else { return [] }
        var out: [String] = []
        let done = expectation(description: "debug/verbs")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let verbs = json["verbs"] as? [String] {
                out = verbs
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return out
    }

    /// Make the stub refuse one verb, so a failure path can be driven.
    private func refuseVerb(_ verb: String) {
        guard let url = URL(string: "http://localhost:6039/debug/refuse-verb/\(verb)")
        else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let done = expectation(description: "refuse")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
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
