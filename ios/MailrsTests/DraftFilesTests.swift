import Foundation
import Testing

@testable import Mailrs

@MainActor
struct DraftFilesTests {
    private static let part = MultipartForm.FilePart(
        name: "attachments", filename: "a.txt", contentType: "text/plain", data: Data("abc".utf8))
    private static let stored = Wire.DraftAttachment(
        blobId: "Uabc", name: "a.txt", contentType: "text/plain", size: 3)

    private struct Refused: Error, LocalizedError {
        var errorDescription: String? { "the message is larger than the 25 MiB limit" }
    }

    @Test func anUploadedFileIsStoredAndNeedsASave() async {
        let files = DraftFiles()
        await files.add(Self.part) { _ in Self.stored }
        #expect(files.stored == [Self.stored])
        #expect(files.unsaved.isEmpty)
        #expect(files.needsSave)
        files.saved([Self.stored])
        #expect(!files.needsSave)
    }

    /// A refused file stays, with the server's words, and still goes with
    /// a send from this window.
    @Test func aRefusedFileKeepsTheReasonAndTravelsInTheForm() async {
        let files = DraftFiles()
        await files.add(Self.part) { _ in throw Refused() }
        #expect(files.stored.isEmpty)
        #expect(files.unsaved == [Self.part])
        #expect(files.entries.first?.refusal == "the message is larger than the 25 MiB limit")
        #expect(!files.needsSave)
    }

    @Test func aFileRemovedWhileUploadingStaysRemoved() async {
        let files = DraftFiles()
        await files.add(Self.part) { _ in
            await MainActor.run {
                if let id = files.entries.first?.id { files.remove(id) }
            }
            return Self.stored
        }
        #expect(files.entries.isEmpty)
    }

    /// A reopened draft's files are what the server already names.
    @Test func restoredFilesNeedNoSaveUntilOneIsRemoved() {
        let files = DraftFiles()
        files.restore([Self.stored, Self.stored])
        #expect(!files.needsSave)
        #expect(files.stored.count == 2)
        if let id = files.entries.first?.id { files.remove(id) }
        #expect(files.needsSave)
        #expect(files.stored == [Self.stored])
    }

    /// Removing every file from a saved draft is a change the server must
    /// hear, or it attaches them to the send.
    @Test func removingTheLastFileNeedsASave() {
        let files = DraftFiles()
        files.restore([Self.stored])
        for entry in files.entries { files.remove(entry.id) }
        #expect(files.needsSave)
    }
}
