import XCTest

/// The sectioned Inbox and team rows: a section shows its own rows, a team
/// conversation names who answers it, and New's senders can be accepted.
final class SectionFlowTests: MailrsUITestCase {

    func testASectionShowsItsOwnRowsAndATeamRowNamesItsAssignee() {
        let app = launch(signedIn: true)
        let reply = element(app, "section.reply")
        XCTAssertTrue(reply.waitForExistence(timeout: 10), "no section bar over the Inbox")
        reply.tap()

        let team = element(app, "row.conversation.sec_team")
        XCTAssertTrue(team.waitForExistence(timeout: 10), "the To reply section did not show its rows")
        XCTAssertTrue(element(app, "row.assignee.sec_team").exists, "the team row did not say who answers it")
        XCTAssertFalse(element(app, "row.assignee.sec_reply").exists, "a row nobody is assigned named an assignee")

        element(app, "section.awaiting").tap()
        XCTAssertTrue(element(app, "row.conversation.sec_wait").waitForExistence(timeout: 10),
                      "Waiting for a reply did not show its row")
        XCTAssertFalse(element(app, "row.conversation.sec_team").exists, "the last section's rows stayed on screen")
    }

    func testAcceptAllAcceptsTheShownSendersAndTheyLeaveNew() {
        let app = launch(signedIn: true)
        let new = element(app, "section.new")
        XCTAssertTrue(new.waitForExistence(timeout: 10), "no section bar over the Inbox")
        new.tap()
        XCTAssertTrue(element(app, "row.conversation.sec_new1").waitForExistence(timeout: 10),
                      "New did not show its first contacts")

        let accept = element(app, "section.acceptAll")
        XCTAssertTrue(accept.waitForExistence(timeout: 5), "New offered no Accept all")
        accept.tap()

        let gone = NSPredicate(format: "exists == false")
        expectation(for: gone, evaluatedWith: element(app, "row.conversation.sec_new1"))
        waitForExpectations(timeout: 10)
        XCTAssertEqual(Set(accepted()), ["new@first.test", "second@first.test"],
                       "Accept all did not send each shown sender, bare and lower-cased")
    }

    /// The senders the app asked the stub to accept.
    private func accepted() -> [String] {
        guard let url = URL(string: "http://localhost:6039/debug/accepted") else { return [] }
        var result: [String] = []
        let done = expectation(description: "debug/accepted")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data, let list = try? JSONSerialization.jsonObject(with: data) as? [String] {
                result = list
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }
}
