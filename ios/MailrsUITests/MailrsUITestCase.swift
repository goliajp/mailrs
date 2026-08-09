import XCTest

/// The path a person actually takes: sign in, see the inbox, open a
/// thread.
///
/// Against a stub on localhost rather than a live server — a test that
/// needs someone's real password is a test nobody runs. The stub serves
/// the shapes the Rust handlers send, including a 760px-wide HTML body,
/// so the fit-to-width path is exercised rather than asserted about.
/// What every UI test needs: a launch that pins the language and points
/// at the stub, and the readers that ask the stub what actually arrived.
///
/// A base class rather than a free function so the `XCTestCase` members
/// (`expectation`, `wait`) are in scope. `private` came off the helpers
/// on the way out: in Swift that is file scope, and the subclasses are
/// in other files now.
class MailrsUITestCase: XCTestCase {
    func launch(
        signedIn: Bool = false, folder: String? = nil, listDelayMs: Int = 0,
        keepCache: Bool = false, language: String? = nil, appearance: String? = nil
    ) -> XCUIApplication {
        resetStub()
        if listDelayMs > 0 { setStubListDelay(listDelayMs) }
        let app = XCUIApplication()
        app.launchArguments = ["-mailrsBaseURL", "http://localhost:6039"]
        // Tests assert empty-start behaviours (spinners, empty states)
        // that yesterday's cached rows would satisfy or contradict at
        // random. The offline test opts out to prove the cache works.
        if !keepCache { app.launchArguments += ["-mailrsFreshCache"] }
        // UserDefaults reads `-key value` pairs off the launch
        // arguments, so a test can set a stored preference without a
        // screen to set it on.
        //
        // English unless a test asks otherwise, because the suite must
        // not depend on the host's language. This simulator runs in
        // Chinese; before the app had a Chinese localization that made
        // no difference, and the moment it did, half the suite went
        // looking for English words on a Chinese screen.
        app.launchArguments += ["-mailrs.language", language ?? "en"]
        // The app's own theme wins over the simulator's appearance —
        // `preferredColorScheme` is an override, and this simulator has
        // "Dark" stored from an earlier run. So `simctl ui appearance
        // light` alone photographed a dark screen. Passing the
        // preference is what actually asks for a light one.
        if let appearance {
            app.launchArguments += ["-mailrs.appearance", appearance]
        }
        // And the process's own language, which is what the system
        // components inside it follow: Quick Look's Done button and the
        // search field's clear button are drawn by iOS, not by this
        // app, and they answered in the simulator's Chinese while the
        // app answered in English. Pinning the app alone left the suite
        // half-translated.
        app.launchArguments += ["-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
        if signedIn {
            app.launchArguments += ["-mailrsToken", "stub-token"]
        } else {
            app.launchArguments += ["-mailrsSignedOut"]
        }
        if let folder { app.launchArguments += ["-mailrsFolder", folder] }
        app.launch()
        return app
    }


    /// Types the credentials and taps through. Shared because both tests
    /// need to be signed in, and a helper that fails loudly beats two
    /// copies drifting apart.
    func signIn(_ app: XCUIApplication) {
        let address = app.textFields["you@example.com"]
        XCTAssertTrue(address.waitForExistence(timeout: 10), "sign-in form never appeared")
        address.tap()
        address.typeText("me@golia.jp")
        let password = app.secureTextFields["Password"]
        password.tap()
        password.typeText("hunter2")
        app.buttons["Sign in"].tap()
        dismissPasswordPrompt()
    }


    /// iOS offers to save the password after a `SecureField` sign-in, and
    /// that prompt belongs to SpringBoard, not to this app — it sits over
    /// the inbox and every row underneath is genuinely untappable while
    /// it is up. Which is what "the row is not hittable" meant, after
    /// three wrong guesses at overlays and modifiers inside the app.
    ///
    /// The labels are localised, so this tries the ones this machine's
    /// simulators actually produce rather than assuming English.
    func dismissPasswordPrompt() {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        // Any button that dismisses it. The dialog is localised — this
        // machine's simulator answers in Chinese — and matching on labels
        // alone was the guess that did not work, so this falls back to
        // "whatever buttons the alert has".
        for label in ["Not Now", "以后", "後で", "Later", "保存", "Save"] {
            let button = springboard.buttons[label]
            if button.waitForExistence(timeout: 2) {
                button.tap()
                return
            }
        }
        let alert = springboard.alerts.firstMatch
        if alert.waitForExistence(timeout: 2), alert.buttons.count > 0 {
            alert.buttons.element(boundBy: 0).tap()
            return
        }
        // Nothing matched — say what was actually there rather than
        // failing three screens later with an unrelated message.
        if springboard.buttons.count > 0 {
            print("=== SPRINGBOARD BUTTONS ===")
            print(springboard.debugDescription)
        }
    }


    /// Make the stub sit on the conversation list, so "first page in
    /// flight" lasts long enough for assertions to look at it.
    func setStubListDelay(_ ms: Int) {
        guard let url = URL(string: "http://localhost:6039/debug/set-delay") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.httpBody = try? JSONSerialization.data(withJSONObject: ["ms": ms])
        let done = expectation(description: "debug/set-delay")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }


    /// Make the stub sit on the conversation list for a while — the
    /// loading state is only observable if loading takes observable time.
    func setStubDelay(_ ms: Int) {
        guard let url = URL(string: "http://localhost:6039/debug/set-delay") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.httpBody = try? JSONSerialization.data(withJSONObject: ["ms": ms])
        let done = expectation(description: "debug/set-delay")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }


    /// Clear the stub's recorders so each test reads only its own
    /// traffic. They are module-level lists in one long-lived process,
    /// so without this "exactly one send" depends on test order.
    /// Switch the reply sheet's segmented mode, verified by the title.
    ///
    /// The sheet auto-focuses the editor, so the keyboard is animating
    /// the form upward exactly when the first tap lands — a tap on
    /// coordinates the segment has already left switches nothing and
    /// raises nothing. The mode change is the assertion; the tap
    /// retries until the title says it happened.
    func switchReplyMode(_ app: XCUIApplication, to mode: String) {
        let segment = app.buttons[mode]
        XCTAssertTrue(segment.waitForExistence(timeout: 5), "no \(mode) segment")
        for _ in 0..<3 {
            segment.tap()
            if app.navigationBars[mode].waitForExistence(timeout: 2) { return }
        }
        XCTFail("the \(mode) segment never switched the sheet")
    }


    /// What reached the account-creation endpoint: the address, and
    /// whether a password came with it.
    func accountPosts() -> [[String: Any]] {
        guard let url = URL(string: "http://localhost:6039/debug/account-posts") else { return [] }
        var result: [[String: Any]] = []
        let done = expectation(description: "debug/account-posts")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let posts = json["posts"] as? [[String: Any]] {
                result = posts
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// What the server currently holds for a group.
    func groupGrants(id: Int) -> [String] {
        guard let url = URL(string: "http://localhost:6039/api/admin/groups/\(id)/permissions") else {
            return []
        }
        var request = URLRequest(url: url)
        request.setValue("Bearer stub-token", forHTTPHeaderField: "Authorization")
        var result: [String] = []
        let done = expectation(description: "group grants")
        URLSession.shared.dataTask(with: request) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let permissions = json["permissions"] as? [String] {
                result = permissions
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// How many times the conversation list has been fetched.
    func listFetches() -> Int {
        guard let url = URL(string: "http://localhost:6039/debug/list-fetches") else { return -1 }
        var result = -1
        let done = expectation(description: "debug/list-fetches")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let fetches = json["fetches"] as? Int {
                result = fetches
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// Every q= the stub's contacts endpoint has answered.
    func contactQueries() -> [String] {
        guard let url = URL(string: "http://localhost:6039/debug/contact-queries") else { return [] }
        var result: [String] = []
        let done = expectation(description: "debug/contact-queries")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let queries = json["queries"] as? [String] {
                result = queries
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// Swipe a row and tap one of the revealed actions.
    ///
    /// The swipe is setup, not the assertion, and under a loaded
    /// machine it sometimes lands as a scroll — the actions never
    /// appear and the test reports a missing button, which reads like
    /// the app lost the action. Retrying until the action exists keeps
    /// the red lights meaningful; if three swipes reveal nothing, that
    /// is a real failure and says so.
    func swipeAndTap(
        _ app: XCUIApplication, row: XCUIElement, edge: SwipeEdge, action: String,
        file: StaticString = #filePath, line: UInt = #line
    ) {
        let button = app.buttons[action]
        for _ in 0..<3 {
            switch edge {
            case .leading: row.swipeRight()
            case .trailing: row.swipeLeft()
            }
            if button.waitForExistence(timeout: 2) {
                button.firstMatch.tap()
                return
            }
        }
        XCTFail("swiping never revealed \(action)", file: file, line: line)
    }


    // Internal, because `swipeAndTap` takes one and is called from
    // the subclasses in the other files.
    enum SwipeEdge { case leading, trailing }


    /// Scroll a sheet until the named button is realized.
    ///
    /// A `List` does not build the cells below the fold, so a button
    /// six rows down does not exist until something scrolls to it —
    /// which is what a reader does too. Without this the failure reads
    /// "no DMARC entry", as though the app had lost the screen.
    func scrollTo(_ app: XCUIApplication, button name: String,
                          file: StaticString = #filePath, line: UInt = #line) {
        scrollTo(app, element: app.buttons[name], named: name, file: file, line: line)
    }


    func scrollTo(_ app: XCUIApplication, element: XCUIElement, named: String,
                          file: StaticString = #filePath, line: UInt = #line) {
        for _ in 0..<6 {
            if element.exists { return }
            app.swipeUp()
        }
        XCTAssertTrue(element.exists, "never scrolled to \(named)", file: file, line: line)
    }


    func resetStub() {
        guard let url = URL(string: "http://localhost:6039/debug/reset") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let done = expectation(description: "debug/reset")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }


    /// How many times anything reached the stub from inside a message.
    func phishHits() -> Int {
        guard let url = URL(string: "http://localhost:6039/debug/phish-hits") else { return -1 }
        var hits = -1
        let done = expectation(description: "phish-hits")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data, let body = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                hits = body["hits"] as? Int ?? -1
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return hits
    }


    /// How many times the badge's count has been fetched.
    func unseenFetches() -> Int {
        guard let url = URL(string: "http://localhost:6039/debug/unseen-fetches") else { return -1 }
        var result = -1
        let done = expectation(description: "debug/unseen-fetches")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let fetches = json["fetches"] as? Int {
                result = fetches
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// Every non-GET the stub has served since the last reset.
    func recordedWrites() -> [String] {
        guard let url = URL(string: "http://localhost:6039/debug/writes") else { return [] }
        var result: [String] = []
        let done = expectation(description: "debug/writes")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let writes = json["writes"] as? [String] {
                result = writes
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// The drafts the stub is holding.
    func storedDrafts() -> [[String: Any]] {
        guard let url = URL(string: "http://localhost:6039/api/mail/drafts") else { return [] }
        var result: [[String: Any]] = []
        let done = expectation(description: "drafts")
        var request = URLRequest(url: url)
        request.setValue("Bearer stub-token", forHTTPHeaderField: "Authorization")
        URLSession.shared.dataTask(with: request) { data, _, _ in
            if let data, let json = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
                result = json
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// The bodies the stub has been POSTed to `/api/mail/send`.
    func sentMessages() -> [[String: Any]] {
        guard let url = URL(string: "http://localhost:6039/debug/sent") else { return [] }
        var result: [[String: Any]] = []
        let done = expectation(description: "debug/sent")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let sent = json["sent"] as? [[String: Any]] {
                result = sent
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// What the stub was asked to unsubscribe from.
    ///
    /// Each entry is the body the client posted. The assertion that
    /// matters is what is **not** in it: no URL. The server takes the
    /// target out of the message's own header, and a client that sent
    /// one would have turned the endpoint into a request forwarder.
    func unsubscribeRequests() -> [[String: Any]] {
        guard let url = URL(string: "http://localhost:6039/debug/unsubscribed") else { return [] }
        var result: [[String: Any]] = []
        let done = expectation(description: "debug/unsubscribed")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let rows = json["unsubscribed"] as? [[String: Any]] {
                result = rows
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }


    /// The attachment indices the stub has served, in order.
    func fetchedAttachmentIndices() -> [Int] {
        guard let url = URL(string: "http://localhost:6039/debug/fetched") else { return [] }
        var result: [Int] = []
        let done = expectation(description: "debug/fetched")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let indices = json["attachment_indices"] as? [Int] {
                result = indices
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }
}
