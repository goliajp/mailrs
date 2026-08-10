import XCTest

/// The path a person actually takes: sign in, see the inbox, open a
/// thread.
///
/// Against a stub on localhost rather than a live server — a test that
/// needs someone's real password is a test nobody runs. The stub serves
/// the shapes the Rust handlers send, including a 760px-wide HTML body,
/// so the fit-to-width path is exercised rather than asserted about.
/// The directory and the operations screens.
///
/// Split out of `SignInFlowTests.swift` — 2,371 lines, in a repository
/// whose 500-line limit did not look at `ios/` until now.
final class AdminFlowTests: MailrsUITestCase {

    /// Permissions are granted by replacing the set, not by sending a
    /// delta.
    ///
    /// The endpoint replaces, so a client that sent only the newly
    /// ticked permission would grant that one and silently revoke
    /// every other — which looks identical on screen. The assertion is
    /// the body that reached the server: both the old grant and the
    /// new one.
    func testGrantingAPermissionSendsTheWholeSet() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Permissions")
        app.buttons["Permissions"].tap()

        XCTAssertTrue(app.staticTexts["Support"].waitForExistence(timeout: 10),
                      "the group list never decoded")
        XCTAssertTrue(app.staticTexts["Built in"].exists,
                      "a builtin group is indistinguishable from an ordinary one")
        // The row, not the words inside it: a NavigationLink's label is
        // one button, and tapping a child static text does not always
        // reach it.
        app.buttons.containing(
            NSPredicate(format: "label CONTAINS %@", "Support")
        ).firstMatch.tap()

        // Buttons, not static texts: each permission row is one
        // button whose label is the permission, and only the ticked
        // one exposes a text of its own — so matching on text finds
        // the checked rows and nothing else.
        XCTAssertTrue(app.buttons["mail.send"].waitForExistence(timeout: 10),
                      "the permission catalogue never loaded")
        scrollTo(app, button: "admin.queue")
        app.buttons["admin.queue"].tap()

        var sent: [String] = []
        for _ in 0..<20 where sent.isEmpty {
            sent = groupGrants(id: 2)
            if sent.count < 2 { sent = [] }
            if sent.isEmpty { Thread.sleep(forTimeInterval: 0.25) }
        }
        XCTAssertEqual(Set(sent), ["mail.read", "admin.queue"],
                       "the grant replaced the set instead of extending it: \(sent)")
    }


    /// The audit log shows what was done, and filters by family.
    ///
    /// The bare action is the shape worth pinning: the server writes
    /// `login` with no dot, and a client that assumed one would show
    /// an empty verb next to a family that is the whole string. The
    /// filter is asserted through the wire — it is a prefix the server
    /// applies over a wider scan, so filtering locally would return
    /// fewer rows than asking for them does.
    func testAuditLogListsAndFiltersByFamily() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Audit log")
        app.buttons["Audit log"].tap()

        XCTAssertTrue(app.staticTexts["old@golia.jp"].waitForExistence(timeout: 10),
                      "the audit log never decoded")
        XCTAssertTrue(app.staticTexts["login"].exists,
                      "an action with no dot lost its verb")

        app.buttons["Filter"].tap()
        app.buttons["alias"].tap()

        XCTAssertTrue(app.staticTexts["sales@golia.jp"].waitForExistence(timeout: 10),
                      "the filtered list never came back")
        XCTAssertFalse(app.staticTexts["login"].exists,
                       "the family filter did not narrow the list")
    }


    /// DMARC reads as deliverability, not as a security score.
    ///
    /// The rate is the assertion, and it is asserted to one decimal:
    /// 158 of 160 is 98.7%, and a screen that rounded it to 99% would
    /// be hiding the two messages a receiver was entitled to reject.
    /// The failing source must also be first — sources that lose mail
    /// are the reason anyone opens this.
    func testDmarcShowsTheAlignmentRateAndTheFailingSource() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        scrollTo(app, button: "DMARC")
        app.buttons["DMARC"].tap()

        XCTAssertTrue(app.staticTexts["98.7%"].waitForExistence(timeout: 10),
                      "the window's alignment rate is wrong or missing")
        XCTAssertTrue(app.staticTexts["158 of 160 messages aligned"].exists,
                      "the rate is shown without what it is a rate of")
        XCTAssertTrue(app.staticTexts["198.51.100.7"].exists,
                      "the source that loses mail is not listed")
        // The published policy, because a perfect rate under p=none
        // means nothing was being enforced.
        XCTAssertTrue(app.staticTexts["p=quarantine"].exists,
                      "the report does not say which policy was published")
    }


    /// The queue answers the question a phone is for: is anything
    /// stuck, and who has the sender given up on.
    ///
    /// The healthy job is the assertion that matters. Its blob has no
    /// error, no attempts and no retry time — a client that required
    /// them would decode nothing at exactly the moment the queue is
    /// fine, and the screen would be empty with nothing to explain it.
    func testQueueShowsStuckMailAndSuppressions() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Queue")
        app.buttons["Queue"].tap()

        XCTAssertTrue(app.staticTexts["stuck@example.com"].waitForExistence(timeout: 10),
                      "the queue never decoded")
        XCTAssertTrue(app.staticTexts["421 too many connections"].exists,
                      "the server's own reason for the failure is not shown")
        XCTAssertTrue(app.staticTexts["fresh@example.com"].exists,
                      "a job with no error and no attempts failed to decode")
        XCTAssertTrue(app.staticTexts["bounced@example.com"].exists,
                      "the suppression list never decoded")

        // The row used to say "Waiting" for both of these, which is the
        // word already at the top of the screen. One is late and one was
        // asked for tomorrow, and an operator has to be able to tell.
        XCTAssertTrue(app.staticTexts["Retrying"].exists,
                      "a job past its retry time still reads as merely waiting")
        XCTAssertTrue(app.staticTexts["Scheduled"].exists,
                      "a scheduled send is indistinguishable from a stuck one")
        XCTAssertTrue(app.staticTexts["3 attempts"].exists,
                      "the attempt count left the row when the timing arrived")

        app.buttons["Clear all"].tap()
        app.buttons["Clear"].tap()
        XCTAssertTrue(app.staticTexts["No suppressed addresses"].waitForExistence(timeout: 10),
                      "the suppressions were not cleared")
        XCTAssertTrue(recordedWrites().contains("DELETE /api/admin/suppressions"),
                      "the clear never reached the server")
    }


    /// Groups list, open, and take a member.
    ///
    /// The two envelopes are the assertion: the group list arrives
    /// under `items` like every other admin collection, and its
    /// members arrive under `members` as bare addresses. A client that
    /// assumed one shape for both would show an empty group with no
    /// error to explain it.
    func testEmailGroupsListAndTakeMembers() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Groups")
        app.buttons["Groups"].tap()

        XCTAssertTrue(app.staticTexts["team@golia.jp"].waitForExistence(timeout: 10),
                      "the group list never decoded — check the items envelope")
        app.staticTexts["team@golia.jp"].tap()

        XCTAssertTrue(app.staticTexts["Keiri"].waitForExistence(timeout: 10),
                      "the members never decoded — they arrive under members, not items")

        app.buttons["Add member"].tap()
        let field = app.textFields["someone@golia.jp"]
        XCTAssertTrue(field.waitForExistence(timeout: 5), "no member prompt")
        field.typeText("press@golia.jp")
        app.buttons["Add"].tap()

        XCTAssertTrue(app.staticTexts["press@golia.jp"].waitForExistence(timeout: 10),
                      "the new member never came back from the server")
        XCTAssertTrue(recordedWrites().contains("POST /api/admin/email-groups/1/members"),
                      "the member was not added on the server")
    }


    /// Accounts are listed and created from the phone.
    ///
    /// The password is the part that matters: it has to reach the
    /// server, which hashes it, and it must not be anywhere else. The
    /// stub records that one arrived, never its value — a recorder
    /// that kept it would put a real password in a debug endpoint the
    /// moment this fixture met a real server.
    func testAccountsCanBeListedAndCreated() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        // Scrolled to, not asserted where it happens to sit: the
        // settings form scrolls, and every row below moves each
        // time a section is added above it.
        scrollTo(app, button: "Accounts")
        app.buttons["Accounts"].tap()

        XCTAssertTrue(app.staticTexts["Li Hao"].waitForExistence(timeout: 10),
                      "the account list never decoded — check the items envelope")
        XCTAssertTrue(app.staticTexts["Inactive"].exists,
                      "an inactive account looks like a live one")

        app.buttons["Add account"].tap()
        let address = app.textFields["account-address"]
        XCTAssertTrue(address.waitForExistence(timeout: 5), "no add form")
        address.tap()
        address.typeText("press@golia.jp")
        app.textFields["account-name"].tap()
        app.textFields["account-name"].typeText("Press")
        app.secureTextFields["account-password"].tap()
        app.secureTextFields["account-password"].typeText("correct horse battery")
        app.buttons["Add"].tap()

        XCTAssertTrue(app.staticTexts["Press"].waitForExistence(timeout: 10),
                      "the new account never came back from the server")
        let posts = accountPosts()
        XCTAssertEqual(posts.last?["address"] as? String, "press@golia.jp")
        XCTAssertEqual(posts.last?["had_password"] as? Bool, true,
                       "the account was created without a password")
    }


    /// Domains are listed and added.
    func testDomainsCanBeListedAndAdded() {
        resetStub()
        let app = launch(signedIn: true)
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")

        app.buttons["Lists"].tap()
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Domains")
        app.buttons["Domains"].tap()

        XCTAssertTrue(app.staticTexts["golia.jp"].waitForExistence(timeout: 10),
                      "the domain list never decoded")
        app.buttons["Add domain"].tap()
        let field = app.textFields["golia.jp"]
        XCTAssertTrue(field.waitForExistence(timeout: 5), "no add prompt")
        field.typeText("golia.example")
        app.buttons["Add"].tap()

        XCTAssertTrue(app.staticTexts["golia.example"].waitForExistence(timeout: 10),
                      "the new domain never came back")
        XCTAssertTrue(recordedWrites().contains("POST /api/admin/domains"),
                      "the domain was not created on the server")
    }
}
