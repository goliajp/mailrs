import XCTest

/// The invitation, on screen.
///
/// Everything under it was already proven: the server resolves the
/// instant, the thread response marks the message, the payload decodes.
/// None of that says the card is *mounted*, and a card that renders
/// nothing looks exactly like a message that carries no calendar part.
/// This is the assertion that can tell them apart.
final class InviteFlowTests: MailrsUITestCase {
    func testAnInvitationRendersItsCard() {
        let app = launch(signedIn: true)
        let row = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(
            app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH %@", "To:"))
                .firstMatch.waitForExistence(timeout: 10),
            "thread never opened")

        XCTAssertTrue(
            app.staticTexts["Product sync"].waitForExistence(timeout: 10),
            "the invitation's summary is not on screen — the card did not mount")

        // The badge that says this is a re-send rather than a first
        // invitation: Exchange does not send METHOD:UPDATE, it raises
        // SEQUENCE, and calling that "New invite" tells the reader the
        // opposite of what happened.
        XCTAssertTrue(app.staticTexts["Updated invite"].exists, "the badge does not say it is an update")

        // The whole timezone argument, in one label: 16:00 in Santa
        // Clara is 08:00 the next morning here, and the organiser's own
        // clock is named beside it because neither number alone is the
        // answer — as the abbreviation in force on 20 August (PDT), not
        // the Windows TZID, which says "Standard" in summer.
        let when = app.staticTexts["invite.when"]
        XCTAssertTrue(when.waitForExistence(timeout: 5), "the card says no time")
        XCTAssertTrue(when.label.contains("PDT"), "the organiser's zone is not named: \(when.label)")
        XCTAssertFalse(when.label.contains("Pacific Standard Time"), "a fixed zone label: \(when.label)")
        XCTAssertTrue(
            when.label.contains("8:00") || when.label.contains("08:00"),
            "the reader's own time is wrong or missing: \(when.label)")

        XCTAssertTrue(app.buttons["invite.join"].exists, "no way to join the meeting")
        XCTAssertTrue(app.buttons["invite.accepted"].exists, "no way to answer it")
    }

    /// An all-day invitation has no instant, only its days; the card says
    /// them, and no time of day. The stub's all-day form runs 3 to 4
    /// November (DTEND is the 5th, the day after the last).
    func testAnAllDayInvitationShowsItsDays() {
        // after launch, which resets the stub; before the thread is opened
        let app = launch(signedIn: true)
        let url = URL(string: "http://localhost:6039/debug/invite-all-day")!
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let armed = expectation(description: "debug/invite-all-day")
        URLSession.shared.dataTask(with: request) { _, _, _ in armed.fulfill() }.resume()
        wait(for: [armed], timeout: 10)
        defer { resetStub() }

        let row = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(app.otherElements["invite.card"].waitForExistence(timeout: 10),
                      "the all-day invitation's card did not mount")

        let when = app.staticTexts["invite.when"]
        XCTAssertTrue(when.waitForExistence(timeout: 5), "the card says no date at all")
        XCTAssertTrue(when.label.contains("Nov 3") && when.label.contains("Nov 4"),
                      "the card does not run from the 3rd to the 4th: \(when.label)")
        XCTAssertFalse(when.label.contains(":"), "an all-day event was given a time: \(when.label)")
    }

    /// A weekly series, as the server annotates it: the card says how it
    /// repeats, and the next occurrence from its start to its end, as the
    /// web does.
    func testARecurringInvitationShowsItsRepeatAndNextOccurrence() {
        let app = openRecurringInvite(language: "en")
        defer { resetStub() }
        let when = app.staticTexts["invite.when"]
        XCTAssertTrue(when.waitForExistence(timeout: 5), "the card says no time")
        XCTAssertTrue(when.label.hasPrefix("Next:"), "not the next occurrence: \(when.label)")
        XCTAssertTrue(when.label.contains(" → "), "the occurrence has no end: \(when.label)")
        // 15:00 Pacific is 07:00 or 08:00 the next morning in Tokyo
        // (the formatter puts a narrow no-break space before PM)
        let organiser = when.label.split(separator: " · ").last.map(String.init) ?? ""
        XCTAssertTrue(organiser.hasPrefix("3:00") && (organiser.hasSuffix("PST") || organiser.hasSuffix("PDT")),
                      "the organiser's clock is missing: \(when.label)")
        let repeats = app.staticTexts["invite.repeat"]
        XCTAssertTrue(repeats.exists, "the card does not say it repeats")
        XCTAssertTrue(repeats.label.hasPrefix("Weekly on Thursday · until "), "\(repeats.label)")
        XCTAssertTrue(app.staticTexts["invite.organizer"].label == "From Chris Dai")
    }

    /// The app set to Japanese on a phone in English, which is how these
    /// tests run: every line of the card is in the app's language, not
    /// only the ones SwiftUI looks up.
    func testTheInvitationCardIsInTheAppsLanguage() {
        let app = openRecurringInvite(language: "ja")
        defer { resetStub() }
        let when = app.staticTexts["invite.when"]
        XCTAssertTrue(when.waitForExistence(timeout: 5), "the card says no time")
        XCTAssertTrue(when.label.hasPrefix("次回："), "the time line is not in Japanese: \(when.label)")
        XCTAssertFalse(when.label.contains("Next") || when.label.contains(" at "), "\(when.label)")
        XCTAssertTrue(when.label.contains(" → "), "the occurrence has no end: \(when.label)")
        let repeats = app.staticTexts["invite.repeat"]
        XCTAssertTrue(repeats.exists, "the card does not say it repeats")
        XCTAssertTrue(repeats.label.hasPrefix("毎週木曜日"), "the repeat line is not in Japanese: \(repeats.label)")
        XCTAssertTrue(repeats.label.hasSuffix("（主催者の時間）"), "\(repeats.label)")
        XCTAssertEqual(app.staticTexts["invite.organizer"].label, "主催者：Chris Dai")
        XCTAssertTrue(app.staticTexts["invite.guests"].label.hasPrefix("ゲスト 3 人 · "),
                      "the guest line is not in Japanese: \(app.staticTexts["invite.guests"].label)")
    }

    /// Signed in, the stub's invitation made a weekly series, the thread
    /// open on it; the reader in Tokyo whatever zone the simulator is in.
    private func openRecurringInvite(language: String) -> XCUIApplication {
        let app = launch(signedIn: true, language: language,
                         extraArguments: ["-mailrs.timeZone", "Asia/Tokyo"])
        // after launch, which resets the stub; before the thread is opened
        let url = URL(string: "http://localhost:6039/debug/invite-recurring")!
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        let armed = expectation(description: "debug/invite-recurring")
        URLSession.shared.dataTask(with: request) { _, _, _ in armed.fulfill() }.resume()
        wait(for: [armed], timeout: 10)

        let row = app.staticTexts["Quarterly report and the follow-up notes"]
        XCTAssertTrue(row.waitForExistence(timeout: 15), "inbox never listed")
        row.tap()
        XCTAssertTrue(app.otherElements["invite.card"].waitForExistence(timeout: 10),
                      "the recurring invitation's card did not mount")
        return app
    }
}
