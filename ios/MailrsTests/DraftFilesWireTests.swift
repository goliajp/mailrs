import Foundation
import Testing

@testable import Mailrs

/// The draft-file bodies against the shared request fixtures in
/// `wire-contract/requests/`, which the server's and the web's suites read
/// too.
struct DraftFilesWireTests {
    private static let quote = Wire.DraftAttachment(
        blobId: "U3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7",
        name: "quote.pdf", contentType: "application/pdf", size: 48213
    )

    private func fixture(_ name: String) throws -> NSDictionary {
        let url = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("wire-contract/requests/\(name).json")
        let data = try Data(contentsOf: url)
        return try #require(try JSONSerialization.jsonObject(with: data) as? NSDictionary)
    }

    private func encoded<T: Encodable>(_ value: T) throws -> NSDictionary {
        let data = try JSONEncoder().encode(value)
        return try #require(try JSONSerialization.jsonObject(with: data) as? NSDictionary)
    }

    @Test func aDraftSaveNamesItsFiles() throws {
        var request = Wire.SaveDraftRequest(
            id: 42, to: "nagata@nagatax.tokyo.jp", cc: "", bcc: "",
            subject: "Quote", body: "The quote is attached.", replyToThreadId: nil
        )
        request.attachments = [Self.quote]
        #expect(try encoded(request) == fixture("draft-save-attachments"))
    }

    /// Absent keeps the draft's files on the server; only a save that
    /// knows them names them.
    @Test func aSaveThatKnowsNoFilesLeavesTheFieldOut() throws {
        let request = Wire.SaveDraftRequest(
            id: 42, to: "", cc: "", bcc: "", subject: "", body: "", replyToThreadId: nil
        )
        #expect(try encoded(request)["attachments"] == nil)
    }

    @Test func aSendOfASavedDraftNamesIt() throws {
        var request = Wire.SendRequest(
            to: ["nagata@nagatax.tokyo.jp"], cc: [], bcc: [],
            subject: "Quote", body: "The quote is attached.",
            inReplyTo: nil, replyToThreadId: nil,
            forwardMessageId: nil, forwardAttachmentsFrom: nil, scheduledAt: nil,
            from: "lihao@golia.jp", htmlBody: "<p>The quote is attached.</p>"
        )
        request.draftId = 42
        #expect(try encoded(request) == fixture("send-draft"))
    }

    @Test func aListedDraftCarriesItsFiles() throws {
        let json = """
        {"id": 7, "to": "", "cc": "", "bcc": "", "subject": "", "body": "",
         "created_at": 1, "updated_at": 2,
         "attachments": [{"blob_id": "Uab", "name": "a.pdf",
                          "content_type": "application/pdf", "size": 3}]}
        """
        let draft = try JSONDecoder().decode(Wire.Draft.self, from: Data(json.utf8))
        #expect(draft.files == [Wire.DraftAttachment(
            blobId: "Uab", name: "a.pdf", contentType: "application/pdf", size: 3)])
    }

    /// The server leaves the field out of a draft with no files.
    @Test func aDraftWithoutFilesHasNone() throws {
        let json = """
        {"id": 7, "to": "", "cc": "", "bcc": "", "subject": "", "body": "",
         "created_at": 1, "updated_at": 2}
        """
        let draft = try JSONDecoder().decode(Wire.Draft.self, from: Data(json.utf8))
        #expect(draft.files.isEmpty)
    }
}
