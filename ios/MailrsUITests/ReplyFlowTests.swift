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
/// Answering and passing on: reply, reply-all, forward.
///
/// Split again at the 500-line limit.
final class ReplyFlowTests: MailrsUITestCase {

    func testTheThreadCanBeMarkedJunk() {
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
        app.buttons["Mark as junk"].tap()

        XCTAssertTrue(row.waitForNonExistence(timeout: 10),
                      "the junked thread is still listed")
        var writes: [String] = []
        for _ in 0..<20 where writes.isEmpty {
            writes = recordedWrites().filter { $0 == "POST /api/conversations/t1/mark-junk" }
            if writes.isEmpty { Thread.sleep(forTimeInterval: 0.25) }
        }
        XCTAssertFalse(writes.isEmpty, "the junk verdict never reached the server")
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
        let toField = app.textFields["composer-to"]
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



    /// A reply with a file rides the multipart route — and must keep
    /// both threading fields. compose.rs defaults each of them, so a
    /// route switch that dropped one would not error; the reply would
    /// arrive detached. The stub records the parsed form, so this is
    /// asserted on what arrived.
    func testAReplyWithAFileKeepsItsThreading() {
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
        editor.typeText("With a file.")
        app.buttons["Attach"].tap()
        app.buttons["Attach sample file"].tap()
        XCTAssertTrue(app.staticTexts["sample.txt"].waitForExistence(timeout: 5),
                      "the attachment row never listed")
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"]
            .waitForExistence(timeout: 10), "the send never dismissed")

        guard let sent = sentMessages().last else {
            XCTFail("nothing reached the server"); return
        }
        let files = sent["attachments"] as? [[String: Any]] ?? []
        XCTAssertEqual(files.first?["filename"] as? String, "sample.txt")
        XCTAssertEqual(sent["in_reply_to"] as? String, "<m2@x>",
                       "the multipart route dropped in_reply_to")
        XCTAssertEqual(sent["reply_to_thread_id"] as? String, "t1",
                       "the multipart route dropped reply_to_thread_id")
    }



    /// A forward with an added file carries both the reference and the
    /// file — the server appends the original and extends the list, so
    /// neither displaces the other.
    func testAForwardWithAFileCarriesReferenceAndFile() {
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

        switchReplyMode(app, to: "Forward")
        let toField = app.textFields["composer-to"]
        XCTAssertTrue(toField.waitForExistence(timeout: 5), "no forward To field")
        toField.tap()
        toField.typeText("third@example.com")
        let editor = app.textViews.firstMatch
        editor.tap()
        editor.typeText("FYI, file added.")
        app.buttons["Attach"].tap()
        app.buttons["Attach sample file"].tap()
        XCTAssertTrue(app.staticTexts["sample.txt"].waitForExistence(timeout: 5))
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"]
            .waitForExistence(timeout: 10), "the send never dismissed")

        guard let sent = sentMessages().last else {
            XCTFail("nothing reached the server"); return
        }
        XCTAssertEqual(sent["forward_message_id"] as? String, "<m2@x>",
                       "the file displaced the forward reference")
        let files = sent["attachments"] as? [[String: Any]] ?? []
        XCTAssertEqual(files.first?["filename"] as? String, "sample.txt",
                       "the reference displaced the file")
        XCTAssertNil(sent["in_reply_to"] ?? nil, "a forward must not thread")
    }
}
