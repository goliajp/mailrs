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
/// Inside a thread: reading it, folding it, walking it, and the cache.
///
/// Split again at the 500-line limit.
final class ThreadFlowTests: MailrsUITestCase {


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
        //
        // An image, not a text: the badge is a mark now, because the
        // words wrapped the header onto two lines. Asserting on its
        // accessibility label is also the check that the words are
        // still said aloud — a mark with no label would look right and
        // tell a VoiceOver reader nothing.
        let badge = app.images["Unverified sender"]
        var swipes = 0
        while !badge.exists && swipes < 8 {
            app.swipeUp()
            swipes += 1
        }
        XCTAssertTrue(badge.exists, "a suspicious sender was shown without saying so")
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

        swipeAndTap(app, row: row, edge: .trailing, action: "Delete")
        // The alert's Delete, not the swipe button behind it.
        app.alerts.buttons["Delete"].tap()

        XCTAssertTrue(row.waitForNonExistence(timeout: 10),
                      "the thread was still listed after a confirmed delete")
    }



    /// Attachments are listed, and tapping one opens it.
    ///
    /// The index the server accepts is the position in the array — there
    /// is none on the wire — so a row that opened the wrong file would
    /// show the wrong preview rather than fail. Tapping the second one
    /// is the check that counting is what the UI does.
    /// The two verdicts you only reach after reading.
    ///
    /// Marking unread is "deal with this later", so it leaves; marking
    /// junk moves the thread out of the list, so it leaves too. Both
    /// are asserted on the wire, since the screen after either one is
    /// the same list.
    func testTheThreadCanBeMarkedUnread() {
        resetStub()
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(app.staticTexts["thread-subject"].waitForExistence(timeout: 10),
                      "thread never opened")

        app.buttons["More"].tap()
        app.buttons["Mark as unread"].tap()

        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].waitForExistence(timeout: 10),
                      "marking unread did not return to the list")
        var writes: [String] = []
        for _ in 0..<20 where writes.isEmpty {
            writes = recordedWrites().filter { $0 == "POST /api/conversations/t1/unread" }
            if writes.isEmpty { Thread.sleep(forTimeInterval: 0.25) }
        }
        XCTAssertFalse(writes.isEmpty, "the unread verdict never reached the server")
    }



    /// Remote images wait to be asked for.
    ///
    /// Fetching one tells the sender the message was opened, from
    /// which address and when — so the newsletter's tracking pixel
    /// must not fire on open. The banner is the affordance; tapping it
    /// is consent for that message only.
    func testRemoteImagesWaitForConsent() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()

        // The newsletter is the folded one; its remote pixel lives in
        // the body the fold hides.
        let folded = app.buttons["collapsed-1"]
        XCTAssertTrue(folded.waitForExistence(timeout: 10), "older message not folded")
        folded.tap()

        let banner = app.buttons["load-images"]
        XCTAssertTrue(banner.waitForExistence(timeout: 10),
                      "a message with a remote pixel loaded it without asking")

        banner.tap()
        XCTAssertTrue(banner.waitForNonExistence(timeout: 5),
                      "the banner outlived the consent")
        // The reply has nothing remote in it and must not be wearing a
        // banner of its own.
        XCTAssertEqual(app.buttons.matching(identifier: "load-images").count, 0,
                       "a local message asked to load images")
    }



    /// Triage without leaving the thread.
    ///
    /// Archiving from inside has to do both halves: reach the server,
    /// and put the reader back on a list that no longer shows the row.
    /// The wire assertion is what separates it from a screen that
    /// merely dismissed.
    func testArchivingFromInsideTheThreadReturnsToAListWithoutIt() {
        resetStub()
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(app.staticTexts["thread-subject"].waitForExistence(timeout: 10),
                      "thread never opened")

        app.buttons["Archive"].tap()

        XCTAssertTrue(row.waitForNonExistence(timeout: 10),
                      "still on the thread, or the row survived the archive")
        XCTAssertTrue(app.staticTexts["請求書のご送付につきまして"].waitForExistence(timeout: 10),
                      "did not return to the list")
        // On the verb, not the path: archiving one thread goes through
        // the same batch call a selection does — one route for one row
        // and for fifty — so the request no longer names the thread in
        // its URL.
        let verbs = postedVerbs()
        XCTAssertTrue(verbs.contains("archive t1"),
                      "the archive never reached the server: \(verbs)")
    }



    /// The star toggles in place — a verdict that does not remove the
    /// thread should not remove the reader from it.
    func testStarringFromInsideTheThreadStays() {
        resetStub()
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        let subject = app.staticTexts["thread-subject"]
        XCTAssertTrue(subject.waitForExistence(timeout: 10), "thread never opened")

        app.buttons["Star"].tap()

        XCTAssertTrue(app.buttons["Unstar"].waitForExistence(timeout: 5),
                      "the button did not become its own undo")
        XCTAssertTrue(subject.exists, "starring left the thread")
        let writes = recordedWrites()
        XCTAssertTrue(writes.contains { $0.hasSuffix("/t1/star") },
                      "the star never reached the server: \(writes)")
    }



    /// The subject is readable in the thread.
    ///
    /// It used to live only in the nav bar, squeezed between a back
    /// button and three toolbar buttons, where it showed six words and
    /// an ellipsis. The count line is the falsifiable part: it exists
    /// only in the header, so a nav-bar title that happens to carry
    /// the full string as its accessibility label cannot satisfy this.
    func testTheThreadShowsItsWholeSubject() {
        let app = launch(signedIn: true)
        let row = app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Quarterly report")
        ).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()

        // The count is the header's own marker — the nav bar never
        // carried it, so this cannot be satisfied by a title.
        XCTAssertTrue(app.staticTexts["2 messages"].waitForExistence(timeout: 10),
                      "no thread header — the subject has nowhere to be read")
        XCTAssertTrue(app.staticTexts["· Alice Smith, spoofed"].exists,
                      "the header lost its participants")
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"].exists,
                      "the header did not carry the subject")
    }



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

        // The header, not the nav bar: the subject moved there when the
        // bar proved too narrow to show it. Asked of the element rather
        // than of the screen — the pushed-away list can still answer
        // that a string exists somewhere.
        let subject = app.staticTexts["thread-subject"]
        var moved = false
        for _ in 0..<20 where !moved {
            moved = subject.exists && subject.label == "請求書のご送付につきまして"
            if !moved { Thread.sleep(forTimeInterval: 0.25) }
        }
        XCTAssertTrue(moved, "the chevron did not move to the next thread")
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
