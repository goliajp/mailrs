import XCTest

/// The path a person actually takes: sign in, see the inbox, open a
/// thread.
///
/// Against a stub on localhost rather than a live server — a test that
/// needs someone's real password is a test nobody runs. The stub serves
/// the shapes the Rust handlers send, including a 760px-wide HTML body,
/// so the fit-to-width path is exercised rather than asserted about.
/// What the app must not do, and what it must say about a sender.
///
/// Split out of `SignInFlowTests.swift` — 2,371 lines, in a repository
/// whose 500-line limit did not look at `ios/` until now.
final class SecurityFlowTests: MailrsUITestCase {

    /// Aliases are listed, added and removed from the phone.
    ///
    /// The request shape is the assertion. The admin list arrives in an
    /// `{items: […]}` envelope while the conversation list is a bare
    /// array, and the add takes four fields including a domain the
    /// server could have derived — a client that guessed either would
    /// look identical on screen and be wrong on the wire.
    func testAliasesCanBeListedAddedAndRemoved() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        XCTAssertTrue(app.buttons["Aliases"].waitForExistence(timeout: 5), "no admin section")
        app.buttons["Aliases"].tap()

        XCTAssertTrue(app.staticTexts["sales@golia.jp"].waitForExistence(timeout: 10),
                      "the alias list never decoded — check the items envelope")
        XCTAssertTrue(app.staticTexts["Inactive"].exists,
                      "an inactive alias is indistinguishable from an active one")

        app.buttons["Add alias"].tap()
        let source = app.textFields["alias-source"]
        XCTAssertTrue(source.waitForExistence(timeout: 5), "no add form")
        source.tap()
        source.typeText("press@golia.jp")
        let target = app.textFields["alias-target"]
        target.tap()
        target.typeText("lihao@golia.jp")
        app.buttons["Add"].tap()

        XCTAssertTrue(app.staticTexts["press@golia.jp"].waitForExistence(timeout: 10),
                      "the new alias never came back from the server")
        let writes = recordedWrites()
        XCTAssertTrue(writes.contains("POST /api/admin/aliases"),
                      "the alias was not created on the server: \(writes)")

        app.staticTexts["press@golia.jp"].swipeLeft()
        app.buttons["Delete"].firstMatch.tap()
        app.buttons["Delete"].firstMatch.tap()
        XCTAssertTrue(app.staticTexts["press@golia.jp"].waitForNonExistence(timeout: 10),
                      "the alias survived its deletion")
    }


    /// Making a key, and the one moment its secret exists.
    ///
    /// The server keeps eight characters of the secret and nothing
    /// else, so the sheet that shows it is the only place it will ever
    /// be. The assertions are the wire (what was asked for) and the
    /// list afterwards (what can still be seen).
    func testAnApiKeyIsMadeAndItsSecretShownOnce() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        app.buttons["API keys"].tap()
        XCTAssertTrue(app.staticTexts["Scheduler"].waitForExistence(timeout: 10),
                      "the key list never decoded")
        XCTAssertTrue(app.staticTexts["mk_a1b2c…"].exists,
                      "a key is recognised by its prefix, and it is not shown")

        app.buttons["New key"].tap()
        let nameField = app.textFields["key-name"]
        XCTAssertTrue(nameField.waitForExistence(timeout: 5), "the new-key sheet never opened")
        nameField.tap()
        nameField.typeText("Reporter")
        // The scope list is the server's catalogue, not a copy.
        app.buttons["mail.read"].tap()
        app.buttons["Create"].tap()

        XCTAssertTrue(app.staticTexts["Copy this key now"].waitForExistence(timeout: 10),
                      "the secret was never shown")
        let secret = app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'mk_'"))
        XCTAssertTrue(secret.firstMatch.exists, "the sheet did not carry the secret")
        // The exact string, because the *prefix* legitimately survives
        // in the list — "anything starting with mk_" would be there
        // afterwards by design, and asserting it is gone was wrong.
        let shown = secret.firstMatch.label
        XCTAssertTrue(shown.count > 12, "that is a prefix, not a secret: \(shown)")
        app.buttons["copy-secret"].tap()
        XCTAssertTrue(app.staticTexts["Copied"].waitForExistence(timeout: 5),
                      "copying said nothing")
        // By identifier: the sheet behind it has a Done of its own, and
        // "the only Done on screen" stopped being true the moment this
        // sheet opened over the list.
        app.buttons["secret-done"].tap()

        // What reached the server, and what remains visible afterwards.
        XCTAssertTrue(recordedWrites().contains("POST /api/agent/keys"),
                      "the key was never created")
        XCTAssertTrue(app.staticTexts["Reporter"].waitForExistence(timeout: 10),
                      "the new key is not in the list")
        XCTAssertFalse(app.staticTexts[shown].exists,
                       "the secret survived the sheet it was shown in")
    }


    /// A message body may not reach the network on its own.
    ///
    /// The fixture is what real mail looks like when it is hostile: a
    /// credential form and a meta refresh, both pointing at the stub.
    /// Neither needs JavaScript, so this client having JavaScript off
    /// stops neither — the previous navigation policy refused link taps
    /// and allowed everything else, which let both straight through.
    ///
    /// The assertion is at the wire, not on the screen: the stub counts
    /// what arrives, and the answer has to be nothing.
    func testAMessageBodyCannotPostOrRedirectOnItsOwn() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.staticTexts["請求書のご送付につきまして"].tap()
        XCTAssertTrue(app.staticTexts["To: Sales <sales@golia.jp>"].waitForExistence(timeout: 10),
                      "the thread never opened")

        // The meta refresh fires on load, with no interaction at all.
        XCTAssertEqual(phishHits(), 0, "the message redirected itself on open")

        // And the form, if its button can be reached, must not post.
        let submit = app.buttons["Sign in to continue"]
        if submit.waitForExistence(timeout: 5) {
            submit.tap()
        }
        XCTAssertEqual(phishHits(), 0, "a form inside a message body posted to the network")

        // The name says Amazon; the mail did not come from Amazon, and
        // the header showed a display name and no address at all. iOS
        // has no phishing API to ask — this is the whole of what an app
        // can do, and it is worth doing.
        XCTAssertTrue(app.staticTexts["mail07.jqjintaiyang.example"].exists,
                      "a display name claiming another domain went unmarked")

        // The signature part is not offered as a file; the real
        // attachment beside it still is.
        XCTAssertFalse(app.staticTexts["smime.p7s"].exists,
                       "an S/MIME signature was listed as an attachment")
        XCTAssertTrue(app.staticTexts["請求書.pdf"].exists,
                      "filtering the signature took the real attachment with it")
    }


    /// Mail that arrived at an alias says which one.
    ///
    /// `sales@` and the signed-in address land in the same mailbox and
    /// looked identical once they got there, so a message written to a
    /// role address gave no sign of it — and that is the fact deciding
    /// whether to answer as a person or as a desk.
    func testMailToAnAliasIsMarkedWithTheAliasItArrivedAt() {
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        // The thread addressed to me directly wears no mark: "via" is
        // only an answer when the direct address is absent.
        app.staticTexts["Quarterly report and the follow-up notes"].tap()
        XCTAssertTrue(app.staticTexts["To: me@golia.jp, Bob <bob@example.com>"]
            .waitForExistence(timeout: 10), "thread never opened")
        XCTAssertFalse(app.staticTexts["sales@golia.jp"].exists,
                       "a directly addressed message claimed to have come via an alias")
        app.navigationBars.buttons.firstMatch.tap()

        app.staticTexts["請求書のご送付につきまして"].tap()
        XCTAssertTrue(app.staticTexts["To: Sales <sales@golia.jp>"].waitForExistence(timeout: 10),
                      "the alias-addressed thread never opened")
        XCTAssertTrue(app.staticTexts["sales@golia.jp"].exists,
                      "mail that arrived at an alias did not name it")
    }
}
