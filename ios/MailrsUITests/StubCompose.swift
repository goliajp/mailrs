import XCTest

extension MailrsUITestCase {
    /// Give the stub account other addresses to send as (`stub_role.py`).
    /// `launch` resets the stub, so call it after.
    func setSendAs(_ addresses: [String]) {
        stubPost("/debug/send-as", json: ["send_as": addresses])
    }

    /// Make the stub answer the next Undo with 409, as the server does once
    /// a held send has left (`stub_compose.py`).
    func makeUndoTooLate() {
        stubPost("/debug/send-late", json: [:])
    }

    /// The message ids the app asked the stub to stop.
    func pendingCancels() -> [String] {
        guard let url = URL(string: "http://localhost:6039/debug/pending-cancels") else { return [] }
        var result: [String] = []
        let done = expectation(description: "debug/pending-cancels")
        URLSession.shared.dataTask(with: url) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let cancels = json["cancels"] as? [String] {
                result = cancels
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }

    /// The files a recorded send carried, by name and byte count: the
    /// form's own, and the ones the stub attached from the draft it named.
    /// A file uploaded when added goes the second way; one the send beat
    /// to the server goes the first.
    func sentFiles(_ sent: [String: Any]) -> [(name: String, bytes: Int)] {
        let form = (sent["attachments"] as? [[String: Any]] ?? []).map {
            (name: $0["filename"] as? String ?? "", bytes: $0["bytes"] as? Int ?? -1)
        }
        let kept = (sent["draft_attachments"] as? [[String: Any]] ?? []).map {
            (name: $0["name"] as? String ?? "", bytes: $0["bytes"] as? Int ?? -1)
        }
        return kept + form
    }

    /// A draft saved as another client would, straight to the stub.
    func storeDraft(_ fields: [String: Any]) {
        stubPost("/api/mail/drafts", json: fields)
    }

    private func stubPost(_ path: String, json: [String: Any]) {
        guard let url = URL(string: "http://localhost:6039" + path) else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("Bearer stub-token", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONSerialization.data(withJSONObject: json)
        let done = expectation(description: path)
        URLSession.shared.dataTask(with: request) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 10)
    }

    /// Open a blank composer from the inbox and address it.
    func openComposer(_ app: XCUIApplication, to address: String) {
        XCTAssertTrue(app.staticTexts["Quarterly report and the follow-up notes"]
            .waitForExistence(timeout: 15), "inbox never listed")
        app.buttons["Compose"].tap()
        let to = app.textFields["someone@example.com"]
        XCTAssertTrue(to.waitForExistence(timeout: 5), "compose never opened")
        to.tap()
        to.typeText(address)
    }
}
