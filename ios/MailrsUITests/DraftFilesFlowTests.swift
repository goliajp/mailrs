import XCTest

/// Files kept with a draft: uploaded when added, named by every save,
/// listed again when the draft is reopened, and attached by the server
/// from the draft a send names.
final class DraftFilesFlowTests: MailrsUITestCase {

    /// Add a file, leave, reopen the draft, open the file, send. The send
    /// carries no file of its own: it names the draft, and the stub attaches
    /// the draft's file — so a client that dropped `draft_id`, or listed
    /// drafts without `with_attachments=true`, fails here.
    func testAFileOutlivesTheComposerAndGoesWithTheSend() {
        let app = launch(signedIn: true)
        openComposer(app, to: "file@example.com")
        app.textFields["composer-subject"].tap()
        app.textFields["composer-subject"].typeText("Quote attached")

        app.buttons["Attach"].tap()
        let sample = app.buttons["Attach sample file"]
        XCTAssertTrue(sample.waitForExistence(timeout: 5), "no attach menu")
        sample.tap()
        XCTAssertTrue(app.staticTexts["sample.txt"].waitForExistence(timeout: 5),
                      "the attachment row never listed")
        XCTAssertTrue(waitForDraftFile(named: "sample.txt"),
                      "no saved draft names the file: \(draftsWithFiles())")

        app.buttons["composer-cancel"].tap()
        app.buttons["Lists"].tap()
        app.buttons["Drafts"].tap()
        let row = app.staticTexts["Quote attached"]
        XCTAssertTrue(row.waitForExistence(timeout: 10), "the draft was not listed")
        row.tap()

        let file = app.staticTexts["sample.txt"]
        XCTAssertTrue(file.waitForExistence(timeout: 5), "the reopened draft lost its file")
        file.tap()
        // Drafts, under the composer, has a Done of its own; Quick Look's is
        // the other one
        let dones = app.buttons.matching(identifier: "Done")
        let opened = NSPredicate(format: "count >= 2")
        XCTAssertEqual(XCTWaiter.wait(for: [expectation(for: opened, evaluatedWith: dones)],
                                      timeout: 15), .completed, "the file did not open")
        XCTAssertFalse(app.staticTexts["Could not open"].exists, "the download failed")
        let draftsDone = app.navigationBars["Drafts"].buttons["Done"].frame
        dones.allElementsBoundByIndex.first { $0.frame != draftsDone }?.tap()
        let closed = NSPredicate(format: "count < 2")
        XCTAssertEqual(XCTWaiter.wait(for: [expectation(for: closed, evaluatedWith: dones)],
                                      timeout: 10), .completed, "the preview did not close")
        XCTAssertTrue(app.textFields["composer-subject"].exists, "closing the preview closed the draft")
        XCTAssertEqual(stubList("/debug/draft-file-gets", key: "gets").count, 1,
                       "opening the file did not read it from the draft file route")

        let draftId = draftsWithFiles().first?["id"] as? Int
        XCTAssertNotNil(draftId, "no draft to send")
        app.buttons["Send"].tap()
        XCTAssertTrue(app.textFields["composer-subject"].waitForNonExistence(timeout: 10),
                      "the send never dismissed")

        guard let sent = sentMessages().last else {
            XCTFail("nothing reached the server"); return
        }
        XCTAssertEqual(sent["draft_id"].map { "\($0)" }, draftId.map { "\($0)" },
                       "the send did not name its draft: \(sent)")
        let fromDraft = sent["draft_attachments"] as? [[String: Any]] ?? []
        XCTAssertEqual(fromDraft.first?["name"] as? String, "sample.txt", "\(sent)")
        XCTAssertEqual(fromDraft.first?["bytes"] as? Int, "sample attachment".utf8.count,
                       "the stored file's bytes did not survive the upload")
        XCTAssertNil(sent["attachments"].flatMap { ($0 as? [Any])?.first },
                     "a file the server already had was uploaded again: \(sent)")
        XCTAssertTrue(draftsWithFiles().isEmpty, "the sent draft is still there")
    }

    /// A file the server refuses says why, in the server's words.
    func testARefusedFileShowsTheServersReason() {
        let app = launch(signedIn: true)
        openComposer(app, to: "file@example.com")
        app.buttons["Attach"].tap()
        let refused = app.buttons["Attach refused sample"]
        XCTAssertTrue(refused.waitForExistence(timeout: 5), "no attach menu")
        refused.tap()
        let reason = app.staticTexts.containing(
            NSPredicate(format: "label CONTAINS %@", "25 MiB limit")).firstMatch
        XCTAssertTrue(reason.waitForExistence(timeout: 5),
                      "the server's reason for refusing the file was not shown")
    }

    private func draftsWithFiles() -> [[String: Any]] {
        stubArray("/api/mail/drafts?with_attachments=true")
    }

    private func waitForDraftFile(named name: String) -> Bool {
        for _ in 0..<40 {
            let names = draftsWithFiles()
                .flatMap { $0["attachments"] as? [[String: Any]] ?? [] }
                .compactMap { $0["name"] as? String }
            if names.contains(name) { return true }
            Thread.sleep(forTimeInterval: 0.25)
        }
        return false
    }

    private func stubList(_ path: String, key: String) -> [Any] {
        let json = stubJSON(path) as? [String: Any]
        return json?[key] as? [Any] ?? []
    }

    private func stubArray(_ path: String) -> [[String: Any]] {
        stubJSON(path) as? [[String: Any]] ?? []
    }

    private func stubJSON(_ path: String) -> Any? {
        guard let url = URL(string: "http://localhost:6039" + path) else { return nil }
        var request = URLRequest(url: url)
        request.setValue("Bearer stub-token", forHTTPHeaderField: "Authorization")
        var result: Any?
        let done = expectation(description: path)
        URLSession.shared.dataTask(with: request) { data, _, _ in
            if let data { result = try? JSONSerialization.jsonObject(with: data) }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return result
    }
}
