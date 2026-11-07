import Foundation

/// Files kept with a draft, between the composers and the client.
@MainActor
extension Session {
    func uploadDraftAttachment(_ file: MultipartForm.FilePart) async throws -> Wire.DraftAttachment {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.uploadDraftAttachment(file)
    }

    func draftAttachment(_ file: Wire.DraftAttachment) async throws -> Data {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.draftAttachment(file)
    }

    /// Add a picked file to `files`, uploading it to the draft file store.
    func attach(_ file: MultipartForm.FilePart, to files: DraftFiles) {
        Task { await files.add(file) { try await self.uploadDraftAttachment($0) } }
    }
}
