import XCTest

/// The path a person actually takes: sign in, see the inbox, open a
/// thread.
///
/// Against a stub on localhost rather than a live server — a test that
/// needs someone's real password is a test nobody runs. The stub serves
/// the shapes the Rust handlers send, including a 760px-wide HTML body,
/// so the fit-to-width path is exercised rather than asserted about.
final class SignInFlowTests: XCTestCase {
    private func launch(
        signedIn: Bool = false, folder: String? = nil, listDelayMs: Int = 0,
        keepCache: Bool = false
    ) -> XCUIApplication {
        resetStub()
        if listDelayMs > 0 { setStubListDelay(listDelayMs) }
        let app = XCUIApplication()
        app.launchArguments = ["-mailrsBaseURL", "http://localhost:6039"]
        // Tests assert empty-start behaviours (spinners, empty states)
        // that yesterday's cached rows would satisfy or contradict at
        // random. The offline test opts out to prove the cache works.
        if !keepCache { app.launchArguments += ["-mailrsFreshCache"] }
        app.launchArguments += signedIn ? ["-mailrsToken", "stub-token"] : ["-mailrsSignedOut"]
        if let folder { app.launchArguments += ["-mailrsFolder", folder] }
        app.launch()
        return app
    }

    /// Types the credentials and taps through. Shared because both tests
    /// need to be signed in, and a helper that fails loudly beats two
    /// copies drifting apart.
    private func signIn(_ app: XCUIApplication) {
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
    private func dismissPasswordPrompt() {
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

    func testSignsInAndListsTheInbox() {
        let app = launch()
        signIn(app)

        // The row is the assertion: reaching the list means the login
        // decoded, the token was kept, and the bare-array conversation
        // response decoded too.
        let firstRow = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(firstRow.waitForExistence(timeout: 15), "inbox never listed")
        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].exists,
                      "the CJK subject did not survive the round trip")
        // The fixture's From is "Alice Smith <alice@example.com>"; the
        // row must wear the name. Seeing the raw form here means the
        // SenderName port silently stopped being applied.
        XCTAssertTrue(app.staticTexts["Alice Smith"].exists,
                      "row shows the raw address, not the display name")
        XCTAssertFalse(app.staticTexts["Alice Smith <alice@example.com>"].exists,
                       "row shows the unparsed From header")
    }

    func testOpensAThreadAndRendersTheBody() {
        // Starts signed in: this test is about opening a thread, and
        // going through the login form summons the system's save-password
        // prompt over the inbox, which nothing here can reach.
        let app = launch(signedIn: true)

        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"].waitForExistence(timeout: 15)
        )
        // The row's tap target is a Button whose accessibility label is
        // the whole row read out — "alice@example.com, Aug 5, Quarterly
        // report…". Neither the cell nor the subject label is hittable on
        // its own, which the hierarchy dump settled after two guesses.
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no tappable row")
        row.tap()

        // "To: …" only exists on the thread screen. `alice@example.com`
        // was the first assertion here and it is also the sender shown in
        // the inbox row, so it passed without anything having navigated —
        // an assertion satisfied by the screen you were already on.
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "thread never opened")

        // The spoofed sender is the second message, below a body authored
        // at 760px — so it has to be scrolled to. The badge is the point
        // of the assertion: a forged From must not borrow the trust of a
        // verified one.
        let badge = app.staticTexts["Unverified sender"]
        var swipes = 0
        while !badge.exists && swipes < 8 {
            app.swipeUp()
            swipes += 1
        }
        XCTAssertTrue(badge.exists, "a suspicious sender was shown without saying so")
    }

    /// Paging must not drop the threads that share the boundary second.
    ///
    /// The stub's paging fixture puts five threads on one timestamp
    /// straddling the 50-row page edge, because the server filters
    /// `latest_date < before_ts` — so a client that asked for its oldest
    /// row's own second would lose the three that did not fit, silently,
    /// and a shorter list looks exactly like the end of the mailbox.
    /// `Paged thread 51` is one of the three.
    func testPagingDoesNotSkipTheBoundarySecond() {
        let app = launch(signedIn: true, folder: "Paged")
        XCTAssertTrue(app.staticTexts["Paged thread 0"].waitForExistence(timeout: 15),
                      "paged list never loaded")

        let target = app.staticTexts["Paged thread 51"]
        var swipes = 0
        while !target.exists && swipes < 40 {
            app.swipeUp(velocity: .fast)
            swipes += 1
        }
        XCTAssertTrue(target.exists, "a thread sharing the page-boundary second was skipped")
    }

    /// Deleting asks first, and a cancel destroys nothing.
    ///
    /// The verb is irreversible — `thread_actions.rs` unlinks the maildir
    /// files, so there is no trash and no undo to offer. The web client
    /// reached the same verb from a swipe without asking until
    /// 2026-08-05, and one gesture destroyed a thread outright. This app
    /// asks from the first version that can delete.
    func testDeleteAsksBeforeDestroyingAnything() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")

        row.swipeLeft()
        app.buttons["Delete"].firstMatch.tap()

        XCTAssertTrue(app.staticTexts["This will permanently delete all messages."]
            .waitForExistence(timeout: 5), "delete did not ask")
        app.buttons["Cancel"].tap()
        // Still there. The delete is not optimistic — the row leaves only
        // when the server has said it is gone — so a row still on screen
        // is proof nothing was sent.
        XCTAssertTrue(row.waitForExistence(timeout: 5), "the thread went away on a cancel")
    }

    /// Confirming does delete it. The row leaving is the assertion: the
    /// list is not updated optimistically, so it can only have gone after
    /// the server answered.
    func testConfirmingDeleteRemovesTheThread() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")

        row.swipeLeft()
        app.buttons["Delete"].firstMatch.tap()
        // The alert's Delete, not the swipe button behind it.
        app.alerts.buttons["Delete"].tap()

        XCTAssertTrue(row.waitForNonExistence(timeout: 10),
                      "the thread was still listed after a confirmed delete")
    }

    /// Archive does not ask, because it is reversible. A question about a
    /// reversible action is noise that teaches people to dismiss
    /// questions — which is how the one that matters gets dismissed too.
    ///
    /// It lives on the trailing edge now and owns the full swipe — the
    /// triage gesture in both benchmark apps. Delete sits behind it and
    /// deliberately lost the full-swipe slot: the fastest gesture in the
    /// app must not be the irreversible one.
    func testArchiveTakesTheRowWithoutAsking() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")

        row.swipeLeft()
        app.buttons["Archive"].firstMatch.tap()

        XCTAssertFalse(app.staticTexts["This will permanently delete all messages."].exists,
                       "archive asked a question it does not need to")
        XCTAssertTrue(row.waitForNonExistence(timeout: 10),
                      "the thread was still listed after archiving")
    }

    /// The undo snackbar is the other half of archive-without-asking:
    /// the fastest gesture in the app is safe because it can be taken
    /// back on the spot. The wire assertion is the falsifiable part —
    /// a toast that only reinserted the row locally would pass every
    /// screen check while the server still had the thread archived.
    func testUndoBringsAnArchivedRowBack() {
        resetStub()
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")

        row.swipeLeft()
        app.buttons["Archive"].firstMatch.tap()
        XCTAssertTrue(row.waitForNonExistence(timeout: 10), "row not archived")

        let undo = app.buttons["undo-archive"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5), "no undo offered")
        undo.tap()

        XCTAssertTrue(row.waitForExistence(timeout: 10),
                      "undo did not bring the row back")
        XCTAssertFalse(undo.exists, "toast outlived the undo")

        let writes = recordedWrites()
        XCTAssertTrue(writes.contains { $0.hasSuffix("/t1/archive") },
                      "archive never reached the server: \(writes)")
        XCTAssertTrue(writes.contains { $0.hasSuffix("/t1/unarchive") },
                      "undo was local only — the server still has t1 archived: \(writes)")
    }

    /// Select mode is Gmail's batch triage: pick rows, act once. The
    /// batch shares the single undo slot with the swipe — one gesture,
    /// one undo — and the wire assertion requires every selected row to
    /// round-trip, so a batch that only archived the first row, or an
    /// undo that only restored one, cannot pass.
    func testBatchArchiveAndUndoRoundTripEveryRow() {
        resetStub()
        let app = launch(signedIn: true)
        let first = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(first.waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Select"].tap()
        first.tap()
        app.staticTexts["請求書のご送付につきまして"].tap()
        XCTAssertTrue(app.staticTexts["2 selected"].waitForExistence(timeout: 5),
                      "selection did not count both rows")

        app.buttons["Archive"].tap()
        XCTAssertTrue(first.waitForNonExistence(timeout: 10), "batch did not archive")
        XCTAssertTrue(app.staticTexts["Archived ×2"].waitForExistence(timeout: 5),
                      "toast did not carry the batch count")

        app.buttons["undo-archive"].tap()
        XCTAssertTrue(first.waitForExistence(timeout: 10), "first row not restored")
        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].exists,
                      "second row not restored")

        let writes = recordedWrites()
        for tid in ["t1", "t2"] {
            XCTAssertTrue(writes.contains { $0.hasSuffix("/\(tid)/archive") },
                          "\(tid) never archived on the wire: \(writes)")
            XCTAssertTrue(writes.contains { $0.hasSuffix("/\(tid)/unarchive") },
                          "\(tid) never unarchived on the wire: \(writes)")
        }
    }

    /// Sign-out lives in the Lists menu since Select took its toolbar
    /// slot. This test exists because the move briefly deleted the
    /// button outright — nothing was pinning it — and an app you cannot
    /// sign out of fails silently until a human needs to.
    func testSignOutLivesInTheListsMenuAndReturnsToLogin() {
        let app = launch(signedIn: true)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"].waitForExistence(timeout: 15)
        )
        app.buttons["Lists"].tap()
        let signOut = app.buttons["Sign out"]
        XCTAssertTrue(signOut.waitForExistence(timeout: 5), "no sign-out anywhere")
        signOut.tap()
        XCTAssertTrue(app.textFields["you@example.com"].waitForExistence(timeout: 10),
                      "sign out did not return to the login form")
    }

    /// Attachments are listed, and tapping one opens it.
    ///
    /// The index the server accepts is the position in the array — there
    /// is none on the wire — so a row that opened the wrong file would
    /// show the wrong preview rather than fail. Tapping the second one
    /// is the check that counting is what the UI does.
    /// A thread opens with everything but the newest message folded —
    /// the thread is context, the last message is the reason you came.
    /// The fold is real (the older body and its To line are absent, not
    /// just small), and tapping the line brings the full card back.
    func testOlderMessagesOpenFoldedAndUnfoldOnTap() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()

        let folded = app.buttons["collapsed-1"]
        XCTAssertTrue(folded.waitForExistence(timeout: 10), "older message not folded")
        XCTAssertFalse(app.staticTexts["To: me@golia.jp"].exists,
                       "the folded message is still showing its full header")
        XCTAssertFalse(app.staticTexts["請求書_2026年8月分.pdf"].exists,
                       "the folded message is still showing its attachments")

        folded.tap()

        XCTAssertTrue(app.staticTexts["To: me@golia.jp"].waitForExistence(timeout: 5),
                      "tapping the line did not unfold the card")
        XCTAssertTrue(app.staticTexts["請求書_2026年8月分.pdf"].exists,
                      "the unfolded card lost its attachments")
        XCTAssertFalse(folded.exists, "the folded line outlived its card")
    }

    func testOpensAnAttachment() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()

        // The first message opens folded — its attachments live inside
        // the fold, so the header line is the way in.
        let folded = app.buttons["collapsed-1"]
        XCTAssertTrue(folded.waitForExistence(timeout: 10), "older message not collapsed")
        folded.tap()

        XCTAssertTrue(app.staticTexts["請求書_2026年8月分.pdf"].waitForExistence(timeout: 10),
                      "the attachment was not listed")
        XCTAssertTrue(app.staticTexts["logo.png"].exists, "the second attachment was not listed")

        app.staticTexts["logo.png"].tap()

        // Quick Look presents in a navigation controller so the sheet has
        // a way out; Done appearing means the preview opened rather than
        // the download failing into an alert.
        // Done, not the Quick Look overlay: the preview opened without a
        // close button until 2026-08-07, leaving dragging the sheet down
        // as the only way out.
        let done = app.buttons["Done"]
        XCTAssertTrue(done.waitForExistence(timeout: 15),
                      "the preview opened with no way to close it")
        XCTAssertFalse(app.staticTexts["Could not open"].exists,
                       "the download failed")

        // Which index was asked for, from the stub rather than from the
        // screen. Nothing on screen can tell them apart — both files
        // preview identically — so the first version of this test passed
        // with every row pinned to index 0, which is the exact bug it
        // exists to catch.
        XCTAssertEqual(fetchedAttachmentIndices(), [1],
                       "the second attachment did not fetch index 1")
        done.tap()
    }

    /// Make the stub sit on the conversation list, so "first page in
    /// flight" lasts long enough for assertions to look at it.
    private func setStubListDelay(_ ms: Int) {
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
    private func setStubDelay(_ ms: Int) {
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
    private func switchReplyMode(_ app: XCUIApplication, to mode: String) {
        let segment = app.buttons[mode]
        XCTAssertTrue(segment.waitForExistence(timeout: 5), "no \(mode) segment")
        for _ in 0..<3 {
            segment.tap()
            if app.navigationBars[mode].waitForExistence(timeout: 2) { return }
        }
        XCTFail("the \(mode) segment never switched the sheet")
    }

    /// Every q= the stub's contacts endpoint has answered.
    private func contactQueries() -> [String] {
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

    private func resetStub() {
        guard let url = URL(string: "http://localhost:6039/debug/reset") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let done = expectation(description: "debug/reset")
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }

    /// How many times the badge's count has been fetched.
    private func unseenFetches() -> Int {
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
    private func recordedWrites() -> [String] {
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
    private func storedDrafts() -> [[String: Any]] {
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
    private func sentMessages() -> [[String: Any]] {
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

    /// The attachment indices the stub has served, in order.
    private func fetchedAttachmentIndices() -> [Int] {
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

    /// Searching narrows to what the server matched, and clearing it
    /// puts the mailbox back.
    func testSearchesAndClears() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        let field = app.searchFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5), "no search field")
        field.tap()
        field.typeText("請求")

        // The Japanese thread matches; the English one does not.
        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].waitForExistence(timeout: 10),
                      "the matching thread was not shown")
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForNonExistence(timeout: 5),
                      "a thread the server did not match stayed on screen")

        // Clearing restores the list rather than leaving the results up.
        app.buttons["Clear text"].firstMatch.tap()
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 10), "the mailbox did not come back")
    }

    /// Search results keep the server's ranking.
    ///
    /// `search_conversations` hydrates by walking the ranked hit ids, so
    /// the array arrives in relevance order — re-sorting it by date on
    /// the client throws the ranking away. The stub returns two hits with
    /// the OLDER one first for exactly this reason: with a single hit,
    /// a date sort and a preserved ranking are indistinguishable, and an
    /// earlier version of this test passed with the client sorting.
    func testSearchKeepsTheServersOrder() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        let field = app.searchFields.firstMatch
        field.tap()
        field.typeText("ref")

        let older = app.staticTexts["請求書のご送付につきまして"]
        let newer = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(older.waitForExistence(timeout: 10), "search returned nothing")
        XCTAssertTrue(newer.exists, "search dropped a hit")

        // The server ranked the older thread first. On screen it must be
        // above the newer one.
        XCTAssertLessThan(older.frame.minY, newer.frame.minY,
                          "the results were re-sorted by date, discarding the ranking")
    }

    /// One character is not a search. The server's own LIKE stage refuses
    /// it, so asking is a round trip that can only return noise — and the
    /// stub answers a match for "e" precisely so a client without the
    /// floor would visibly narrow the list.
    func testOneCharacterDoesNotSearch() {
        let app = launch(signedIn: true)
        let english = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(english.waitForExistence(timeout: 15), "inbox never listed")

        let field = app.searchFields.firstMatch
        field.tap()
        field.typeText("e")

        // Both rows still there: nothing was searched.
        Thread.sleep(forTimeInterval: 1.5)
        XCTAssertTrue(english.exists, "one character narrowed the list")
        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].exists,
                      "one character narrowed the list")
    }

    /// Switching lists asks the server for the new list, and takes the
    /// old list's rows with it.
    ///
    /// The stub gives each list a distinct row precisely so a switcher
    /// that changed the title without changing the request would be
    /// visible here — that is the shape this would fail as.
    func testSwitchingListsChangesWhatIsListed() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Junk"].tap()

        XCTAssertTrue(app.staticTexts["You have won"].waitForExistence(timeout: 10),
                      "Junk did not load its own threads")
        XCTAssertFalse(app.staticTexts["Quarterly report and the follow-up notes"].exists,
                       "the previous list's rows stayed on screen")
        XCTAssertTrue(app.navigationBars["Junk"].waitForExistence(timeout: 5),
                      "the title did not follow the list")
    }

    /// Archived is cross-folder — it names no folder and sets a flag —
    /// so it is the list most likely to be built wrong.
    func testArchivedIsItsOwnList() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Archived"].tap()

        XCTAssertTrue(app.staticTexts["Archived thread"].waitForExistence(timeout: 10),
                      "Archived did not ask for archived threads")
    }

    /// A search belongs to the list it was typed in.
    ///
    /// iOS 26 presents `.searchable` over the bottom of the screen and
    /// takes the whole navigation bar away while it is up, so a list
    /// cannot be switched mid-search — the path a user has is to dismiss
    /// the search first. `Session.select` clears the query and its
    /// results anyway, because "the rows on screen belong to the list on
    /// screen" should not depend on which of two screens is in front.
    func testSwitchingListsAfterASearchShowsTheNewList() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        let field = app.searchFields.firstMatch
        field.tap()
        field.typeText("請求")
        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].waitForExistence(timeout: 10),
                      "search did not run")

        app.buttons["close"].tap()

        app.buttons["Lists"].tap()
        app.buttons["Junk"].tap()

        XCTAssertTrue(app.staticTexts["You have won"].waitForExistence(timeout: 10),
                      "Junk did not load")
        XCTAssertFalse(app.staticTexts["請求書のご送付につきまして"].exists,
                       "a result from the previous list's search survived the switch")
    }

    /// Starring shows immediately and survives the round trip.
    ///
    /// The star indicator is the assertion rather than the swipe button's
    /// label: the label flips off local state either way, so a toggle
    /// that never reached the server would still relabel itself. The row
    /// carrying a star is what a user sees.
    func testStarringShowsOnTheRow() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        XCTAssertFalse(row.label.contains("Starred"), "the fixture already starts starred")

        row.swipeRight()
        app.buttons["Star"].firstMatch.tap()

        let starred = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Starred")
        ).firstMatch
        XCTAssertTrue(starred.waitForExistence(timeout: 10),
                      "the row did not show a star after starring")
    }

    /// Marking read clears the unread marker.
    ///
    /// Asserted on the row's spoken label, which is the only place the
    /// state exists as anything but a colour — the dot had no
    /// accessibility label until 2026-08-07, so this test had nothing to
    /// read and its first version only checked the row was still there.
    func testMarkingReadClearsTheUnreadMarker() {
        let app = launch(signedIn: true)
        // Both the subject and "Unread": once the row is read its swipe
        // action relabels itself to "Unread", and a locator matching that
        // word alone finds the button instead of the row.
        let unread = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@ AND label CONTAINS %@",
                        "Quarterly report", "Unread")
        ).firstMatch
        XCTAssertTrue(unread.waitForExistence(timeout: 15),
                      "no unread row to mark read")

        unread.swipeRight()
        app.buttons["Read"].firstMatch.tap()

        XCTAssertTrue(unread.waitForNonExistence(timeout: 10),
                      "the row is still announced as unread")
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"].exists,
                      "marking read removed the row")
    }

    /// Typing a partial name offers the contact book; tapping the
    /// offer lands the bare address on the wire. The floor is the
    /// falsifiable part: a one-character token must never reach the
    /// server, so the stub's query log distinguishes a debounced
    /// autocomplete from one that fires per keystroke.
    func testContactSuggestionCompletesTheToField() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["New message"].tap()
        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 5), "compose never opened")
        to.tap()
        to.typeText("ali")

        let offer = app.buttons["Alice Smith <alice@example.com>"]
        XCTAssertTrue(offer.waitForExistence(timeout: 5), "no suggestion offered")
        offer.tap()

        XCTAssertEqual(to.value as? String, "alice@example.com, ",
                       "the pick did not land as the addr-spec")
        XCTAssertFalse(offer.exists, "the offer outlived the pick")

        app.buttons["Send"].tap()
        XCTAssertTrue(to.waitForNonExistence(timeout: 10), "the send never dismissed")

        guard let sent = sentMessages().last else {
            XCTFail("nothing reached the server"); return
        }
        XCTAssertEqual(sent["to"] as? [String], ["alice@example.com"])

        let queries = contactQueries()
        XCTAssertFalse(queries.contains("a"),
                       "a one-character token was queried: \(queries)")
        XCTAssertTrue(queries.contains { $0.hasPrefix("al") },
                      "the typed token never reached the contacts endpoint: \(queries)")
    }

    /// A compose with a file goes out the multipart door, and the file
    /// arrives — filename, declared type, and its actual bytes counted
    /// server-side. The stub records what it parsed out of the form,
    /// so an encoder that drops the CRLF before the boundary (the file
    /// silently truncates) or misquotes the filename (the field parses
    /// as empty) fails here rather than on a real mailbox.
    func testAnAttachedFileArrivesThroughTheMultipartSend() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["New message"].tap()
        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 5), "compose never opened")
        to.tap()
        to.typeText("file@example.com")

        app.buttons["Attach"].tap()
        let sample = app.buttons["Attach sample file"]
        XCTAssertTrue(sample.waitForExistence(timeout: 5), "no attach menu")
        sample.tap()

        XCTAssertTrue(app.staticTexts["sample.txt"].waitForExistence(timeout: 5),
                      "the attachment row never listed")

        app.buttons["Send"].tap()
        XCTAssertTrue(to.waitForNonExistence(timeout: 10), "the send never dismissed")

        guard let sent = sentMessages().last else {
            XCTFail("nothing reached the server"); return
        }
        XCTAssertEqual(sent["to"] as? [String], ["file@example.com"])
        let files = sent["attachments"] as? [[String: Any]] ?? []
        XCTAssertEqual(files.count, 1, "expected one file: \(sent)")
        XCTAssertEqual(files.first?["filename"] as? String, "sample.txt")
        XCTAssertEqual(files.first?["content_type"] as? String, "text/plain")
        XCTAssertEqual(files.first?["bytes"] as? Int, "sample attachment".utf8.count,
                       "the file's bytes did not survive the form encoding")
    }

    /// Removing an attachment before sending means it does not travel —
    /// the send falls back to the JSON route, which has no file field.
    func testARemovedAttachmentDoesNotTravel() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["New message"].tap()
        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 5), "compose never opened")
        to.tap()
        to.typeText("file@example.com")
        app.buttons["Attach"].tap()
        app.buttons["Attach sample file"].tap()
        XCTAssertTrue(app.staticTexts["sample.txt"].waitForExistence(timeout: 5))

        app.buttons["Remove sample.txt"].tap()
        XCTAssertTrue(app.staticTexts["sample.txt"].waitForNonExistence(timeout: 5),
                      "the row outlived the removal")

        app.buttons["Send"].tap()
        XCTAssertTrue(to.waitForNonExistence(timeout: 10), "the send never dismissed")
        guard let sent = sentMessages().last else {
            XCTFail("nothing reached the server"); return
        }
        XCTAssertNil(sent["attachments"], "a removed file still travelled: \(sent)")
    }

    /// A new message starts its own thread.
    ///
    /// The threading fields are the assertion, and they are read from
    /// the stub because nothing on screen shows them — a compose that
    /// filed its message inside whatever conversation was open would look
    /// identical here and be wrong on the server. Sending
    /// `reply_to_thread_id` on a new message is the mirror of the bug
    /// that made replies arrive detached.
    func testComposesANewMessageWithNoThread() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["New message"].tap()

        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 5), "compose never opened")
        // Send stays disabled until there is somewhere to send it.
        XCTAssertFalse(app.buttons["Send"].isEnabled, "Send was offered with no recipient")

        to.tap()
        to.typeText("someone@example.com")
        app.textFields["Subject"].tap()
        app.textFields["Subject"].typeText("Hello")
        app.textViews.firstMatch.tap()
        app.textViews.firstMatch.typeText("First contact.")

        XCTAssertTrue(app.buttons["Send"].isEnabled, "Send stayed disabled with a valid address")
        app.buttons["Send"].tap()

        // Back on the list: the sheet dismisses only on a send the server
        // said it queued.
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 10), "the compose sheet never dismissed")

        let sent = sentMessages()
        XCTAssertEqual(sent.count, 1, "expected exactly one send")
        XCTAssertEqual(sent.first?["to"] as? [String], ["someone@example.com"])
        XCTAssertEqual(sent.first?["subject"] as? String, "Hello")
        XCTAssertNil(sent.first?["in_reply_to"] as? String,
                     "a new message named a parent message")
        XCTAssertNil(sent.first?["reply_to_thread_id"] as? String,
                     "a new message was filed inside a thread")
        // Sent means no longer a draft. The sheet's closing save runs
        // after the send's delete, and without the didSend guard it
        // quietly refiled the just-deleted draft from the fields still
        // in memory — the reply sheet shipped that exact bug first.
        XCTAssertEqual(storedDrafts().count, 0,
                       "the sent compose left a draft behind")
    }

    /// Closing the composer keeps what was written, and reopening it
    /// from Drafts gets it back.
    ///
    /// Cancel is not discard. Before drafts, dismissing the sheet threw
    /// the message away with no warning and no way back.
    func testACancelledComposeIsKeptAsADraft() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["New message"].tap()
        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 5), "compose never opened")
        to.tap()
        to.typeText("later@example.com")
        app.textFields["Subject"].tap()
        app.textFields["Subject"].typeText("Half written")

        app.buttons["Cancel"].tap()

        app.buttons["Lists"].tap()
        app.buttons["Drafts"].tap()
        XCTAssertTrue(app.staticTexts["Half written"].waitForExistence(timeout: 10),
                      "the cancelled compose was not kept")

        app.staticTexts["Half written"].tap()
        let resumedSubject = app.textFields["Subject"]
        XCTAssertTrue(resumedSubject.waitForExistence(timeout: 5), "the draft did not reopen")
        XCTAssertEqual(resumedSubject.value as? String, "Half written",
                       "the draft reopened without its subject")
    }

    /// One compose session is one draft, however long it is typed for.
    ///
    /// The server upserts on a supplied id; a client that posted without
    /// its id would leave a draft per autosave tick. The stub models
    /// that, so a trail shows up here as a count.
    func testAutosaveKeepsOneDraftPerSession() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["New message"].tap()
        let subject = app.textFields["Subject"]
        XCTAssertTrue(app.textFields["someone@example.com"].waitForExistence(timeout: 5))
        subject.tap()
        subject.typeText("One")
        // Past the 2s autosave, twice, with an edit in between.
        Thread.sleep(forTimeInterval: 3)
        subject.typeText(" and two")
        Thread.sleep(forTimeInterval: 3)
        app.buttons["Cancel"].tap()

        app.buttons["Lists"].tap()
        app.buttons["Drafts"].tap()
        XCTAssertTrue(app.staticTexts["One and two"].waitForExistence(timeout: 10),
                      "the draft did not keep the latest text")
        // From the server, not `app.cells`: the conversation list stays
        // mounted under the sheet, so its two rows are counted too and
        // one draft reads as three.
        XCTAssertEqual(storedDrafts().count, 1,
                       "autosave left more than one draft for a single compose")
    }

    /// An untouched composer leaves nothing behind. A Drafts list filling
    /// up with blanks is worse than no list.
    func testAnEmptyComposeSavesNothing() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["New message"].tap()
        XCTAssertTrue(app.textFields["someone@example.com"].waitForExistence(timeout: 5))
        app.buttons["Cancel"].tap()

        Thread.sleep(forTimeInterval: 2)
        XCTAssertTrue(storedDrafts().isEmpty, "an empty compose left a draft behind")
    }

    /// Opening a thread marks it read — once, and only because it was
    /// opened.
    ///
    /// Asserted at the network level as well as on the row: the read verb
    /// answers 204 with no body, so a client that fired it twice, or from
    /// the wrong place, would look identical on screen. The web client
    /// shipped exactly that bug — a hidden pane marked the newest thread
    /// read on arrival, twice, without it ever being displayed.
    func testOpeningAThreadMarksItRead() {
        let app = launch(signedIn: true)
        let unreadRow = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@ AND label CONTAINS %@",
                        "Quarterly report", "Unread")
        ).firstMatch
        XCTAssertTrue(unreadRow.waitForExistence(timeout: 15), "no unread row")

        // Arriving at the mailbox marks nothing. The list selecting or
        // drawing a row is not reading it.
        XCTAssertFalse(recordedWrites().contains { $0.hasSuffix("/read") },
                       "something marked mail read before any thread was opened")

        unreadRow.tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "thread never opened")

        var writes: [String] = []
        for _ in 0..<20 {
            writes = recordedWrites().filter { $0.hasSuffix("/read") }
            if !writes.isEmpty { break }
            Thread.sleep(forTimeInterval: 0.25)
        }
        XCTAssertEqual(writes, ["POST /api/conversations/t1/read"],
                       "expected exactly one read for the opened thread")

        // And the row shows it: back on the list, the unread marker is
        // gone but the row itself is not.
        app.navigationBars.buttons.firstMatch.tap()
        XCTAssertTrue(unreadRow.waitForNonExistence(timeout: 10),
                      "the row is still announced as unread after being read")
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"].exists,
                      "reading the thread removed the row")
    }

    /// Marking junk sends the verdict and takes the row off the list.
    ///
    /// The verb is the assertion (network level, like read): mark-junk
    /// trains the Bayes filter, so a menu item that archived instead
    /// would look right on screen and silently teach the filter nothing.
    func testMarkAsJunkSendsTheVerdictAndRemovesTheRow() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")

        row.press(forDuration: 1.0)
        let markJunk = app.buttons["Mark as junk"]
        XCTAssertTrue(markJunk.waitForExistence(timeout: 5), "no junk item in the menu")
        markJunk.tap()

        XCTAssertTrue(row.waitForNonExistence(timeout: 10),
                      "the junked thread stayed in the Inbox")
        var writes: [String] = []
        for _ in 0..<20 {
            writes = recordedWrites().filter { $0.contains("junk") }
            if !writes.isEmpty { break }
            Thread.sleep(forTimeInterval: 0.25)
        }
        XCTAssertEqual(writes, ["POST /api/conversations/t1/mark-junk"],
                       "the junk verdict never reached the server")
    }

    /// In the Junk list the same gesture offers the rescue instead.
    func testJunkListOffersNotJunk() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        app.buttons["Lists"].tap()
        app.buttons["Junk"].tap()
        let junkRow = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "You have won")
        ).firstMatch
        XCTAssertTrue(junkRow.waitForExistence(timeout: 10), "junk list never loaded")

        junkRow.press(forDuration: 1.0)
        XCTAssertTrue(app.buttons["Not junk"].waitForExistence(timeout: 5),
                      "the Junk list did not offer the rescue")
        XCTAssertFalse(app.buttons["Mark as junk"].exists,
                       "the Junk list offered junking what is already junk")
        app.buttons["Not junk"].tap()

        XCTAssertTrue(junkRow.waitForNonExistence(timeout: 10),
                      "the rescued thread stayed in Junk")
        XCTAssertTrue(recordedWrites().contains("POST /api/conversations/junk1/mark-not-junk"),
                      "the rescue never reached the server")
    }

    /// The Send list shows all three kinds of row honestly: a filed
    /// message with its status joined across a bracket difference, a
    /// failed send the maildir sweep has not filed, and old mail with no
    /// projection saying nothing.
    func testSendListJoinsStatusAndShowsFailures() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Send"].tap()

        // The failed, unfiled send is the row the list exists for — a
        // naive /api/mail/sent listing would not contain it at all.
        XCTAssertTrue(app.staticTexts["Never left the queue"].waitForExistence(timeout: 10),
                      "the unfiled failed send is missing — the list is only showing the sent axis")
        XCTAssertTrue(app.staticTexts["Failed"].exists, "the failure is not called out")

        // The filed one carries its delivered mark, joined across the
        // bracket difference the stub deliberately serves.
        XCTAssertTrue(app.staticTexts["Filed and delivered"].exists, "the filed send is missing")

        // And the pre-projection one is present with no badge — absence
        // says nothing rather than claiming delivery.
        XCTAssertTrue(app.staticTexts["Predates the projection"].exists,
                      "old mail without a projection row was dropped")
    }

    /// The badge's count is refreshed when the mailbox may have moved.
    ///
    /// The icon itself belongs to the OS — no test can read it — so what
    /// is pinned is the input: the server count is fetched on arrival,
    /// and fetched again after reading a thread changes it. A badge fed
    /// once at launch goes stale the moment mail is read, which is the
    /// number a person then distrusts forever.
    func testBadgeCountRefreshesAfterReading() {
        let app = launch(signedIn: true)
        let unreadRow = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@ AND label CONTAINS %@",
                        "Quarterly report", "Unread")
        ).firstMatch
        XCTAssertTrue(unreadRow.waitForExistence(timeout: 15), "inbox never listed")

        let onArrival = unseenFetches()
        XCTAssertGreaterThan(onArrival, 0, "the badge count was never fetched at all")

        unreadRow.tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "thread never opened")

        var after = onArrival
        for _ in 0..<20 {
            after = unseenFetches()
            if after > onArrival { break }
            Thread.sleep(forTimeInterval: 0.25)
        }
        XCTAssertGreaterThan(after, onArrival,
                             "reading a thread did not refresh the badge count")
    }

    /// While the first page is in flight, the screen says "loading",
    /// never "empty".
    ///
    /// "All caught up" flashing on every open — announced about a full
    /// mailbox, retracted a beat later when the rows arrived — was the
    /// state before this. An empty state is a conclusion; showing it
    /// without the evidence teaches people the screen lies, and neither
    /// Apple Mail nor Gmail ever shows it during a load.
    /// The offline promise: a relaunch opens on the rows the last
    /// session saw, before the network answers. The delayed stub is
    /// what makes it falsifiable — without the cache, nothing can put
    /// a row on screen until the 6-second answer arrives, so the
    /// 3-second wait only passes if the rows came from disk.
    func testARelaunchOpensOnCachedRowsBeforeTheNetworkAnswers() {
        let first = launch(signedIn: true)
        XCTAssertTrue(first.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "the caching launch never listed")
        first.terminate()

        setStubListDelay(6000)
        let app = XCUIApplication()
        app.launchArguments = ["-mailrsBaseURL", "http://localhost:6039",
                               "-mailrsToken", "stub-token"]
        app.launch()
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 3),
            "no rows before the delayed network answer — the cache did not serve")
        app.terminate()
        setStubListDelay(0)
    }

    /// The other half of offline: a conversation opened before is
    /// readable on relaunch while the network is still 6 seconds away.
    /// The 3-second wait is the falsifiable part — only the disk can
    /// paint the body inside it.
    func testAnOpenedThreadIsReadableFromCacheOnRelaunch() {
        let first = launch(signedIn: true)
        let row = first.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(first.staticTexts["To: me@golia.jp, Bob <bob@example.com>"]
            .waitForExistence(timeout: 10), "thread never opened on the caching launch")
        first.terminate()

        setStubListDelay(6000)
        let app = XCUIApplication()
        app.launchArguments = ["-mailrsBaseURL", "http://localhost:6039",
                               "-mailrsToken", "stub-token"]
        app.launch()
        let cachedRow = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(cachedRow.waitForExistence(timeout: 3), "no cached rows")
        cachedRow.tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"]
            .waitForExistence(timeout: 3),
            "the thread did not paint from disk before the network answer")
        // Hand the stub back without the delay: the app under test may
        // still hold requests parked in 6-second sleeps, and the next
        // test should not inherit them.
        app.terminate()
        setStubListDelay(0)
    }

    func testLoadingShowsProgressNotTheEmptyState() {
        let app = launch(signedIn: true, listDelayMs: 2500)

        XCTAssertTrue(app.activityIndicators.firstMatch.waitForExistence(timeout: 2),
                      "no loading indicator during the first page")
        XCTAssertFalse(app.staticTexts["All caught up"].exists,
                       "the empty state was shown while the first page was still in flight")

        // And the conclusion arrives: rows, not the spinner.
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "the list never loaded")
        XCTAssertFalse(app.activityIndicators.firstMatch.exists,
                       "the spinner outlived the load")
    }

    /// The thread view's chevrons walk the list without leaving it.
    ///
    /// Serial processing is the throughput feature: without it every
    /// message costs a round trip through the list. The step is an open,
    /// so it marks read through the same rule as any open — asserted via
    /// the stub's write log, since t2's read is invisible on this screen.
    func testChevronsWalkToTheNextThread() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "thread never opened")
        XCTAssertFalse(app.buttons["Previous thread"].isEnabled,
                       "the first thread offered a previous")

        app.buttons["Next thread"].tap()

        XCTAssertTrue(app.navigationBars["請求書のご送付につきまして"].waitForExistence(timeout: 10),
                      "the chevron did not move to the next thread")
        XCTAssertFalse(app.buttons["Next thread"].isEnabled,
                       "the last thread offered a next")
        var writes: [String] = []
        for _ in 0..<20 {
            writes = recordedWrites().filter { $0 == "POST /api/conversations/t2/read" }
            if !writes.isEmpty { break }
            Thread.sleep(forTimeInterval: 0.25)
        }
        XCTAssertEqual(writes.count, 1, "stepping to a thread did not mark it read")
    }

    /// Reply All widens the To line to everyone on the last message
    /// minus me. The fixture's last message carries a third party
    /// precisely so this is distinguishable from Reply — with identical
    /// To lines, a reply-all that only hit the sender would pass.
    func testReplyAllAddressesEveryoneButMe() {
        resetStub()
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "thread never opened")
        app.buttons["Reply"].tap()

        switchReplyMode(app, to: "Reply All")
        let editor = app.textViews.firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 5), "no composer")
        editor.tap()
        editor.typeText("All hands answer.")
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "the sheet never dismissed")

        guard let sent = sentMessages().last else {
            XCTFail("nothing reached the server"); return
        }
        let to = sent["to"] as? [String] ?? []
        XCTAssertEqual(Set(to), ["spoofed@example.com", "bob@example.com"],
                       "reply-all did not address sender plus the third party: \(to)")
        XCTAssertFalse(to.contains("me@golia.jp"), "reply-all addressed me back")
        XCTAssertNotNil(sent["in_reply_to"] as? String, "reply-all lost threading")
    }

    /// Forward is the backend kind: the typed text travels, the original
    /// body and attachments are appended server-side from the .eml. The
    /// wire shape is the assertion — `forward_message_id` present, both
    /// threading fields absent (a forward starts its own thread).
    func testForwardSendsByReferenceWithoutThreading() {
        resetStub()
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "thread never opened")
        app.buttons["Reply"].tap()

        switchReplyMode(app, to: "Forward")
        let toField = app.textFields["forward-to"]
        XCTAssertTrue(toField.waitForExistence(timeout: 5), "no forward To field")
        toField.tap()
        toField.typeText("third@example.com")
        let editor = app.textViews.firstMatch
        editor.tap()
        editor.typeText("FYI.")
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "the sheet never dismissed")

        guard let sent = sentMessages().last else {
            XCTFail("nothing reached the server"); return
        }
        XCTAssertEqual(sent["to"] as? [String], ["third@example.com"])
        XCTAssertEqual(sent["forward_message_id"] as? String, "<m2@x>",
                       "forward did not reference the original message")
        XCTAssertTrue((sent["subject"] as? String ?? "").hasPrefix("Fwd:"),
                      "forward subject not prefixed")
        XCTAssertNil(sent["in_reply_to"] ?? nil, "a forward must not thread")
        XCTAssertNil(sent["reply_to_thread_id"] ?? nil, "a forward must not thread")
    }

    /// A half-written reply survives: closing the sheet files it under
    /// the thread, reopening puts the words back, sending clears it.
    /// The wire's reply_to_thread_id is what separates a reply draft
    /// from a loose compose draft — without it, resume could never
    /// find the thread again.
    func testAHalfWrittenReplySurvivesTheSheetClosing() {
        resetStub()
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"]
            .waitForExistence(timeout: 10), "thread never opened")

        app.buttons["Reply"].tap()
        let editor = app.textViews.firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 5), "no composer")
        editor.tap()
        editor.typeText("Half a reply")
        app.buttons["Cancel"].tap()

        // The draft reached the server, filed under this thread.
        let stored = storedDrafts()
        XCTAssertEqual(stored.count, 1, "expected exactly one draft: \(stored)")
        XCTAssertEqual(stored.first?["reply_to_thread_id"] as? String, "t1",
                       "the reply draft lost its thread")

        // Reopening the sheet resumes where the typing stopped.
        app.buttons["Reply"].tap()
        XCTAssertTrue(editor.waitForExistence(timeout: 5), "sheet never reopened")
        let resumed = app.textViews.containing(
            NSPredicate(format: "value CONTAINS %@", "Half a reply")
        ).firstMatch
        XCTAssertTrue(resumed.waitForExistence(timeout: 5),
                      "the reply draft did not restore into the editor")

        editor.tap()
        editor.typeText(" — finished.")
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"]
            .waitForExistence(timeout: 10), "the send never dismissed the sheet")

        // Sent means no longer a draft.
        XCTAssertEqual(storedDrafts().count, 0,
                       "the sent reply left its draft behind")
        XCTAssertTrue((sentMessages().last?["body"] as? String ?? "").contains("Half a reply"),
                      "the sent body lost the restored half")
    }

    func testRepliesToAThread() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()

        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "thread never opened")
        app.buttons["Reply"].tap()

        // The subject is prefilled from the thread and not editable here,
        // so the only thing to type is the message.
        let editor = app.textViews.firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 5), "no composer")
        editor.tap()
        editor.typeText("Noted, thanks.")

        app.buttons["Send"].tap()

        // Back on the thread: the sheet dismisses only on a send the
        // server said it queued. If it had failed the sheet would still
        // be up with the reason in it.
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"].waitForExistence(timeout: 10),
                      "the reply sheet never dismissed — the send did not succeed")

        // The other side of the same coin: a reply must carry both
        // threading fields, since `compose.rs` defaults each of them and
        // a missing one arrives as a detached message rather than an
        // error.
        let sent = sentMessages()
        XCTAssertEqual(sent.count, 1, "expected exactly one send")
        XCTAssertEqual(sent.first?["reply_to_thread_id"] as? String, "t1")
        // The newest message in the thread, not the first — a reply
        // answers what was last said. The stub's thread has two.
        XCTAssertEqual(sent.first?["in_reply_to"] as? String, "<m2@x>")
    }
}
