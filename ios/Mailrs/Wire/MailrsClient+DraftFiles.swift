import Foundation

extension Wire {
    /// Backend: `crates/core-api/src/method/admin/userdata.rs` —
    /// `DraftAttachment`. A file kept with a draft: the bytes are in the
    /// account's upload store under `blobId`, the draft holds this.
    struct DraftAttachment: Codable, Hashable, Sendable {
        /// `U` and the SHA-256 of the bytes, as the upload answers it.
        let blobId: String
        let name: String
        let contentType: String
        let size: Int64

        enum CodingKeys: String, CodingKey {
            case blobId = "blob_id"
            case name
            case contentType = "content_type"
            case size
        }
    }
}

/// Files kept with a draft. A file goes into the server's upload store the
/// moment it is added; the draft then names it by `blob_id`.
extension MailrsClient {

    /// `POST /api/mail/draft-attachments` — one file field. Backend:
    /// `crates/webapi/src/handlers/draft_attachments.rs` — `upload`. A
    /// refusal (too large, uploads full, too many at once) comes back in
    /// the server's own words.
    func uploadDraftAttachment(_ file: MultipartForm.FilePart) async throws -> Wire.DraftAttachment {
        let boundary = "mailrs-\(UUID().uuidString)"
        let part = MultipartForm.FilePart(
            name: "file", filename: file.filename, contentType: file.contentType, data: file.data)
        let form = MultipartForm.encode(fields: [], files: [part], boundary: boundary)
        let (data, response) = try await send(
            "POST", url: url("/api/mail/draft-attachments"), body: form, authorized: true,
            contentType: "multipart/form-data; boundary=\(boundary)"
        )
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response")
        }
        guard (200..<300).contains(http.statusCode) else {
            if let reason = Self.errorCode(data) { throw MailrsError.transport(reason) }
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode(Wire.DraftAttachment.self, from: data)
        } catch {
            throw MailrsError.decoding("draft attachment — \(error)")
        }
    }

    /// `GET /api/mail/draft-attachments/{blob_id}` — the caller's own
    /// upload; anyone else's is not found.
    func draftAttachment(_ file: Wire.DraftAttachment) async throws -> Data {
        let path = "/api/mail/draft-attachments/\(MailrsClient.segment(file.blobId))"
        let (data, response) = try await send("GET", path, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        return data
    }
}
