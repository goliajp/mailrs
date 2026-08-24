import Foundation
import XCTest

/// The whole of it, through the screens: add a mailbox somewhere else,
/// fetch from it, write a message, send it — against a **real mail
/// server over TLS**.
///
/// `MailboxFlowTests` checks everything up to the connection, because
/// when it was written there was no IMAP to connect to. There is now:
/// `ios/Testing/tls-mail-stub.py`, reached over a real socket with a
/// certificate the simulator has been told to trust (the app is not
/// modified; see `scripts/ios-build.sh`). So the last hole in this
/// repo's own coverage accounting closes here — **no test on any
/// platform had ever gone through "write → send" in the interface.**
///
/// Skipped when the stub is not listening, so a run from Xcode without
/// the script does not report a failure about a server that was never
/// started.
final class RealMailFlowTests: MailrsUITestCase {
    private let imaps = 9993
    private let submission = 9587

    /// A plain TCP connect, because a UI test bundle runs **in the
    /// simulator** and cannot start a process on the host. The
    /// simulator shares the host's network stack, so this reaches the
    /// same listener the app will.
    private func stubIsUp(_ port: Int) -> Bool {
        let sock = socket(AF_INET, SOCK_STREAM, 0)
        guard sock >= 0 else { return false }
        defer { close(sock) }
        var address = sockaddr_in()
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = UInt16(port).bigEndian
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        return withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(sock, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) == 0
            }
        }
    }

    private func openMailboxes(_ app: XCUIApplication) {
        XCTAssertTrue(app.buttons["Lists"].waitForExistence(timeout: 15), "no Lists button")
        app.buttons["Lists"].tap()
        XCTAssertTrue(app.buttons["Settings"].waitForExistence(timeout: 10), "no Settings row")
        app.buttons["Settings"].tap()
        scrollTo(app, button: "Mailboxes")
        app.buttons["Mailboxes"].tap()
        XCTAssertTrue(
            app.textFields["account.address"].waitForExistence(timeout: 10),
            "the mailboxes sheet never opened")
    }

    private func fill(_ field: XCUIElement, _ text: String) {
        field.tap()
        // Cleared first: the manual fields open **filled in** with what
        // the app guessed, and typing appends to a guess.
        if let existing = field.value as? String, !existing.isEmpty {
            field.press(forDuration: 1.0)
            if app0().menuItems["Select All"].waitForExistence(timeout: 2) {
                app0().menuItems["Select All"].tap()
            }
        }
        field.typeText(text)
    }

    private func app0() -> XCUIApplication { XCUIApplication() }

    /// Add the account through the form, then fetch, then send.
    func testAMailboxIsAddedFetchedFromAndSentThrough() throws {
        try XCTSkipUnless(stubIsUp(imaps), "the TLS mail stub is not listening")
        let app = launch(signedIn: true)
        openMailboxes(app)

        let address = app.textFields["account.address"]
        address.tap()
        address.typeText("me@example.com")

        XCTAssertTrue(
            app.secureTextFields["account.secret"].waitForExistence(timeout: 5),
            "no secret was asked for")
        app.secureTextFields["account.secret"].tap()
        app.secureTextFields["account.secret"].typeText("app-password")

        scrollTo(app, button: "Enter the server settings myself")
        app.buttons["Enter the server settings myself"].tap()

        fill(app.textFields["account.incoming.host"], "127.0.0.1")
        fill(app.textFields["account.incoming.port"], "\(imaps)")
        fill(app.textFields["account.outgoing.host"], "127.0.0.1")
        fill(app.textFields["account.outgoing.port"], "\(submission)")

        scrollTo(app, button: "Add")
        app.buttons["account.add"].tap()

        // The form closes on success and stays open with a reason on
        // failure — so asserting the reason is absent is not the same
        // as asserting it worked, and both are checked.
        XCTAssertFalse(
            app.staticTexts["account.failure"].waitForExistence(timeout: 20),
            "the server refused: \(app.staticTexts["account.failure"].label)")
        // The added account appears as its own row in the same sheet.
        // Asserting on the row rather than on the form having closed
        // says the account was **kept**, which is the thing that has
        // to be true for anything after this to work.
        let id = accountId(for: "me@example.com")
        XCTAssertTrue(
            app.otherElements["account.\(id)"].waitForExistence(timeout: 20)
                || app.staticTexts["me@example.com"].waitForExistence(timeout: 5),
            "the server accepted the credential but no account was kept")

        // --- and now write one and send it ---------------------------
        //
        // The compose sheet dismissing is a fact about the sheet. What
        // has to be true is that a message crossed the socket, and the
        // only place that is knowable is the server — so this asks it.
        let before = receivedCount()

        // Out of the accounts sheet and into the mail itself: two
        // different screens, reached from the same settings section
        // because a screen reached from nowhere is a screen nobody
        // finds.
        app.buttons["accounts.done"].tap()
        scrollTo(app, button: "Other mail")
        app.buttons["Other mail"].tap()
        XCTAssertTrue(
            app.buttons["mailboxes.sync"].waitForExistence(timeout: 15)
                || app.buttons["mail.compose"].waitForExistence(timeout: 5),
            "the mail list never appeared after adding the account")
        XCTAssertTrue(
            app.buttons["mail.compose"].waitForExistence(timeout: 10),
            "there was no way to write a message")
        app.buttons["mail.compose"].tap()

        let to = app.textFields["compose.to"]
        XCTAssertTrue(
            to.waitForExistence(timeout: 10),
            "the composer never opened. Bars: "
                + app.navigationBars.allElementsBoundByIndex.map { $0.identifier }
                .joined(separator: "/")
                + " fields: "
                + app.textFields.allElementsBoundByIndex.map {
                    $0.identifier.isEmpty ? "(\($0.placeholderValue ?? "?"))" : $0.identifier
                }.joined(separator: ", "))
        to.tap()
        to.typeText("you@example.com")
        app.textFields["compose.subject"].tap()
        app.textFields["compose.subject"].typeText("Lunch")
        app.textViews["compose.body"].tap()
        app.textViews["compose.body"].typeText("Half twelve?")
        app.buttons["compose.send"].tap()

        XCTAssertFalse(
            app.staticTexts["compose.failure"].waitForExistence(timeout: 20),
            "the send failed: \(app.staticTexts["compose.failure"].label)")

        var arrived = before
        for _ in 0..<40 where arrived == before {
            Thread.sleep(forTimeInterval: 0.5)
            arrived = receivedCount()
        }
        XCTAssertEqual(
            arrived, before + 1,
            "the composer said it sent, and nothing reached the server")
        XCTAssertTrue(
            lastReceived().contains("Subject: Lunch"),
            "what arrived was not the message that was written")
    }

    /// How many messages the mail stub has taken, over its plain-HTTP
    /// window. Not TLS: this is the test asking the server what it saw,
    /// which is a different conversation from the one under test.
    private func receivedCount() -> Int { probe()["count"] as? Int ?? -1 }

    private func lastReceived() -> String { probe()["last"] as? String ?? "" }

    private func probe() -> [String: Any] {
        guard let url = URL(string: "http://127.0.0.1:9995/received"),
            let data = try? Data(contentsOf: url),
            let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return [:] }
        return json
    }

    /// `MailAccount.id(for:)`, spelled again here because a UI test
    /// bundle cannot import the app's types. A drift between the two
    /// makes this look for a row that exists under another name, so
    /// the fallback above is on the address rather than on nothing.
    private func accountId(for address: String) -> String {
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        for scalar in address.lowercased().unicodeScalars {
            hash ^= UInt64(scalar.value)
            hash = hash &* 0x100_0000_01b3
        }
        return "acct-\(hash)"
    }
}
