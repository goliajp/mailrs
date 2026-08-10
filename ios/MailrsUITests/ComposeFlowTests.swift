import XCTest

/// The path a person actually takes: sign in, see the inbox, open a
/// thread.
///
/// Against a stub on localhost rather than a live server — a test that
/// needs someone's real password is a test nobody runs. The stub serves
/// the shapes the Rust handlers send, including a 760px-wide HTML body,
/// so the fit-to-width path is exercised rather than asserted about.
/// Writing: compose, reply, forward, drafts, attachments, sending.
///
/// Split out of `SignInFlowTests.swift` — 2,371 lines, in a repository
/// whose 500-line limit did not look at `ios/` until now.
final class ComposeFlowTests: MailrsUITestCase {


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
        app.textFields["composer-subject"].tap()
        app.textFields["composer-subject"].typeText("Hello")
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
        app.textFields["composer-subject"].tap()
        app.textFields["composer-subject"].typeText("Half written")

        app.buttons["Cancel"].tap()

        app.buttons["Lists"].tap()
        app.buttons["Drafts"].tap()
        XCTAssertTrue(app.staticTexts["Half written"].waitForExistence(timeout: 10),
                      "the cancelled compose was not kept")

        app.staticTexts["Half written"].tap()
        let resumedSubject = app.textFields["composer-subject"]
        XCTAssertTrue(resumedSubject.waitForExistence(timeout: 5), "the draft did not reopen")
        XCTAssertEqual(resumedSubject.value as? String, "Half written",
                       "the draft reopened without its subject")
    }



    /// A draft is the least recoverable thing in the app, and it was the
    /// only destructive action with neither a confirmation nor an undo —
    /// while deleting an alias, which takes five seconds to retype,
    /// asked twice.
    func testDeletingADraftAsksAndCanBeRefused() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["New message"].tap()
        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 5), "compose never opened")
        to.tap()
        to.typeText("later@example.com")
        app.textFields["composer-subject"].tap()
        app.textFields["composer-subject"].typeText("Worth keeping")
        app.buttons["Cancel"].tap()

        app.buttons["Lists"].tap()
        app.buttons["Drafts"].tap()
        XCTAssertTrue(app.staticTexts["Worth keeping"].waitForExistence(timeout: 10),
                      "the draft was not kept")

        let row = app.staticTexts["Worth keeping"]
        swipeAndTap(app, row: row, edge: .trailing, action: "Delete")
        let alert = app.alerts["Delete draft?"]
        XCTAssertTrue(alert.waitForExistence(timeout: 5), "the delete was not confirmed at all")
        // The alert names it, because a list of half-written messages is
        // exactly where "this draft" is not enough.
        XCTAssertTrue(alert.staticTexts["Worth keeping"].exists,
                      "the alert did not name the draft it would delete")
        alert.buttons["Cancel"].tap()
        XCTAssertTrue(row.waitForExistence(timeout: 5),
                      "refusing the confirmation still deleted the draft")
        XCTAssertEqual(storedDrafts().count, 1, "a refused delete reached the server")

        swipeAndTap(app, row: row, edge: .trailing, action: "Delete")
        app.alerts["Delete draft?"].buttons["Delete"].tap()
        XCTAssertTrue(app.staticTexts["No drafts"].waitForExistence(timeout: 10),
                      "the confirmed delete did not happen")
        XCTAssertEqual(storedDrafts().count, 0, "the draft outlived its deletion")
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
        let subject = app.textFields["composer-subject"]
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



    /// The same claim, in the sheet that did not inherit it.
    ///
    /// Drafts was written after the mail lists and had no loading gate:
    /// it announced "No drafts" the instant it opened and then filled in
    /// underneath itself. A sheet is not exempt from a rule the screen
    /// behind it follows.
    func testDraftsWaitsBeforeSayingThereAreNone() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        // A draft to be found, so "No drafts" would be a false claim
        // rather than a true one arriving early.
        app.buttons["New message"].tap()
        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 5), "compose never opened")
        to.tap()
        to.typeText("later@example.com")
        app.textFields["composer-subject"].tap()
        app.textFields["composer-subject"].typeText("Slow to arrive")
        app.buttons["Cancel"].tap()

        // Relaunched, because the flash only happens on a *cold* open:
        // the compose sheet's own save left the drafts in memory, so
        // opening the sheet in the same session has data to show and is
        // right to show it. The first attempt at this test asserted a
        // spinner that correctly was not there.
        setStubListDelay(2500)
        app.terminate()
        app.launch()

        // Wait out the delayed first page, so the spinner asserted below
        // can only be the sheet's own.
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 25), "inbox never listed after relaunch")
        app.buttons["Lists"].tap()
        app.buttons["Drafts"].tap()
        XCTAssertTrue(app.activityIndicators.firstMatch.waitForExistence(timeout: 2),
                      "no loading indicator while the drafts were in flight")
        XCTAssertFalse(app.staticTexts["No drafts"].exists,
                       "the empty state was shown while the request was still out")
        XCTAssertTrue(app.staticTexts["Slow to arrive"].waitForExistence(timeout: 15),
                      "the drafts never arrived")
        app.terminate()
        setStubListDelay(0)
    }
    /// Send is a button, and "send later" is somewhere else.
    ///
    /// It was briefly a `Menu(primaryAction:)` labelled "Send": a
    /// button to a person, a menu to anything driving the app. Every
    /// test that taps Send then waited out its timeout for a message
    /// the menu had swallowed, and a suite that had been taking ten
    /// minutes stopped finishing at all.
    func testSendIsATapAndSchedulingIsItsOwnControl() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(
            app.staticTexts["Quarterly report and the follow-up notes"]
                .waitForExistence(timeout: 15), "inbox never listed")
        app.buttons["New message"].tap()

        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 10), "composer never opened")
        to.tap()
        to.typeText("alice@example.com")

        XCTAssertTrue(app.buttons["Send later"].exists,
                      "no way to schedule — send later has no control of its own")
        app.buttons["Send"].tap()

        // Sent, not menued: the stub records the body, and a menu that
        // opened instead would leave nothing here.
        var sent: [[String: Any]] = []
        for _ in 0..<20 {
            sent = sentMessages()
            if !sent.isEmpty { break }
            Thread.sleep(forTimeInterval: 0.25)
        }
        XCTAssertEqual(sent.count, 1, "tapping Send did not send")
        XCTAssertNil(sent.first?["scheduled_at"],
                     "an ordinary send carried a schedule")
    }

}
