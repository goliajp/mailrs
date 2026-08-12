import XCTest

/// The path a person actually takes: sign in, see the inbox, open a
/// thread.
///
/// Against a stub on localhost rather than a live server — a test that
/// needs someone's real password is a test nobody runs. The stub serves
/// the shapes the Rust handlers send, including a 760px-wide HTML body,
/// so the fit-to-width path is exercised rather than asserted about.
/// Reading: the list, a thread, search, triage, and the cache.
///
/// Split out of `SignInFlowTests.swift` — 2,371 lines, in a repository
/// whose 500-line limit did not look at `ios/` until now.
/// The list itself: which one is shown, searching it, paging it.
///
/// Split again at the 500-line limit.
final class ListFlowTests: MailrsUITestCase {

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

        swipeAndTap(app, row: row, edge: .trailing, action: "Archive")
        XCTAssertTrue(row.waitForNonExistence(timeout: 10), "row not archived")

        let undo = app.buttons["undo-archive"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5), "no undo offered")
        undo.tap()

        XCTAssertTrue(row.waitForExistence(timeout: 10),
                      "undo did not bring the row back")
        XCTAssertFalse(undo.exists, "toast outlived the undo")

        // Asserted as verbs rather than URLs. A one-row swipe now takes
        // the same road as a fifty-row selection — one `/batch` request,
        // one undo slot, one refusal path — so the path no longer names
        // the thread. What must hold is that the server was told to
        // archive t1 and then told to put it back; that is true of
        // either transport, and it is the thing the reader depends on.
        let verbs = postedVerbs()
        XCTAssertTrue(verbs.contains("archive t1"),
                      "archive never reached the server: \(verbs)")
        XCTAssertTrue(verbs.contains("unarchive t1"),
                      "undo was local only — the server still has t1 archived: \(verbs)")
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



    /// Coming back to the app asks for the mail again.
    ///
    /// Until push is live this is the only thing that makes a return
    /// show new mail — without it the list is whatever it was when you
    /// left. The stub's fetch count is the assertion: a screen that
    /// merely reappeared would leave it where it was.
    func testReturningToTheAppRefreshesTheList() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        let onOpen = listFetches()
        XCTAssertGreaterThan(onOpen, 0, "the launch never fetched")

        XCUIDevice.shared.press(.home)
        app.activate()
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "the app did not come back")

        var after = onOpen
        for _ in 0..<20 where after == onOpen {
            after = listFetches()
            if after == onOpen { Thread.sleep(forTimeInterval: 0.25) }
        }
        XCTAssertGreaterThan(after, onOpen,
                             "returning to the app did not ask for the mail again")
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
}
