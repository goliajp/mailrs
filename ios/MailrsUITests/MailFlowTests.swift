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
final class MailFlowTests: MailrsUITestCase {


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

        swipeAndTap(app, row: row, edge: .trailing, action: "Delete")

        XCTAssertTrue(app.staticTexts["This will permanently delete all messages."]
            .waitForExistence(timeout: 5), "delete did not ask")
        app.buttons["Cancel"].tap()
        // Still there. The delete is not optimistic — the row leaves only
        // when the server has said it is gone — so a row still on screen
        // is proof nothing was sent.
        XCTAssertTrue(row.waitForExistence(timeout: 5), "the thread went away on a cancel")
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

        swipeAndTap(app, row: row, edge: .trailing, action: "Archive")

        XCTAssertFalse(app.staticTexts["This will permanently delete all messages."].exists,
                       "archive asked a question it does not need to")
        XCTAssertTrue(row.waitForNonExistence(timeout: 10),
                      "the thread was still listed after archiving")
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



    /// The language setting changes the app, not only its dates.
    ///
    /// It shipped as a picker that reformatted numbers while every
    /// word stayed English — the honest half of a setting. The list
    /// title is the assertion because it comes from an enum that
    /// returned `String`, which is exactly the shape that never
    /// reaches a localization table.
    func testTheAppSpeaksTheChosenLanguage() {
        // The process stays English — that is what makes this a test of
        // the in-app override rather than of the simulator's language.
        let app = launch(signedIn: true, language: "zh-Hans")
        XCTAssertTrue(app.staticTexts["收件箱"].waitForExistence(timeout: 15),
                      "the chosen language did not reach the interface")
        XCTAssertFalse(app.staticTexts["Inbox"].exists,
                       "both languages are on screen at once")
        XCTAssertTrue(app.buttons["写邮件"].exists,
                      "the compose button stayed English")
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

        swipeAndTap(app, row: row, edge: .leading, action: "Star")

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

        swipeAndTap(app, row: unread, edge: .leading, action: "Read")

        XCTAssertTrue(unread.waitForNonExistence(timeout: 10),
                      "the row is still announced as unread")
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"].exists,
                      "marking read removed the row")
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
}
