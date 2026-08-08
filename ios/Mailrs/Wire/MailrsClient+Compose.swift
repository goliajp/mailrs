import Foundation

/// Writing: drafts, and the four ways a message leaves.
///
/// Split out of `MailrsClient.swift` at the 500-line limit this
/// repository holds every language to — and which did not look at
/// `ios/` until now. One type, several subjects.
extension MailrsClient {

    /// `POST /api/mail/send`.
    ///
    /// The handler answers 200 with `success: false` for a send it
    /// accepted but could not queue, so the status code alone is not the
    /// answer — a reply that never left would otherwise look sent.
    /// A message that starts its own thread — no `in_reply_to`, no
    /// `reply_to_thread_id`.
    @discardableResult
    func sendNew(
        to recipients: [String], cc: [String] = [], bcc: [String] = [],
        subject: String, body: String
    ) async throws -> Wire.SendResponse {
        try await post(Wire.SendRequest(
            to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
            inReplyTo: nil, replyToThreadId: nil,
            forwardMessageId: nil, forwardAttachmentsFrom: nil
        ))
    }


    @discardableResult
    func sendReply(
        to recipients: [String],
        cc: [String] = [],
        bcc: [String] = [],
        subject: String,
        body: String,
        inReplyTo: String?,
        threadId: String
    ) async throws -> Wire.SendResponse {
        return try await post(Wire.SendRequest(
            to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
            inReplyTo: inReplyTo, replyToThreadId: threadId,
            forwardMessageId: nil, forwardAttachmentsFrom: nil
        ))
    }


    /// `POST /api/mail/send-multipart` — the send that carries files.
    /// Backend: `crates/webapi/src/handlers/send.rs` —
    /// `send_message_multipart`; `to` and `attachments` are repeated
    /// fields, the rest match the JSON names.
    @discardableResult
    func sendMultipart(
        to recipients: [String],
        cc: [String] = [],
        bcc: [String] = [],
        subject: String,
        body: String,
        attachments: [MultipartForm.FilePart],
        inReplyTo: String? = nil,
        replyToThreadId: String? = nil,
        forwardMessageId: String? = nil,
        forwardAttachmentsFrom: UInt32? = nil
    ) async throws -> Wire.SendResponse {
        let boundary = "mailrs-\(UUID().uuidString)"
        // Repeated fields, one per address, exactly as `to` is — the
        // handler pushes each occurrence onto its own vector.
        var fields: [(String, String)] = recipients.map { ("to", $0) }
        fields += cc.map { ("cc", $0) }
        fields += bcc.map { ("bcc", $0) }
        fields.append(("subject", subject))
        fields.append(("body", body))
        // Same optionality contract as the JSON route: absent, never
        // empty — the handler filters empties for some fields and not
        // others, and absent is the shape it always understands.
        if let inReplyTo { fields.append(("in_reply_to", inReplyTo)) }
        if let replyToThreadId { fields.append(("reply_to_thread_id", replyToThreadId)) }
        if let forwardMessageId { fields.append(("forward_message_id", forwardMessageId)) }
        if let forwardAttachmentsFrom {
            fields.append(("forward_attachments_from", String(forwardAttachmentsFrom)))
        }
        let form = MultipartForm.encode(fields: fields, files: attachments, boundary: boundary)
        let url = baseURL.appendingPathComponent("/api/mail/send-multipart")
        let (data, response) = try await send(
            "POST", url: url, body: form, authorized: true,
            contentType: "multipart/form-data; boundary=\(boundary)"
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
        let result: Wire.SendResponse
        do {
            result = try JSONDecoder().decode(Wire.SendResponse.self, from: data)
        } catch {
            throw MailrsError.decoding("send response — \(error)")
        }
        guard result.success else {
            throw MailrsError.transport(result.message ?? "The server did not queue the message.")
        }
        return result
    }


    /// A forward: no threading fields — a forward starts its own thread
    /// — and the original travels by reference, with the server
    /// appending body and attachments from the raw .eml.
    @discardableResult
    func sendForward(
        to recipients: [String],
        cc: [String] = [],
        bcc: [String] = [],
        subject: String,
        body: String,
        forwardMessageId: String,
        forwardAttachmentsFrom: UInt32?
    ) async throws -> Wire.SendResponse {
        return try await post(Wire.SendRequest(
            to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
            inReplyTo: nil, replyToThreadId: nil,
            forwardMessageId: forwardMessageId,
            forwardAttachmentsFrom: forwardAttachmentsFrom
        ))
    }


    /// The one place a compose form becomes a request, so new messages
    /// and replies cannot drift apart in how they read the answer.
    func post(_ payload: Wire.SendRequest) async throws -> Wire.SendResponse {
        let (data, response) = try await send(
            "POST", "/api/mail/send", body: try JSONEncoder().encode(payload), authorized: true
        )
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        let result: Wire.SendResponse
        do {
            result = try JSONDecoder().decode(Wire.SendResponse.self, from: data)
        } catch {
            throw MailrsError.decoding("send response — \(error)")
        }
        guard result.success else {
            throw MailrsError.transport(result.message ?? "The server did not queue the message.")
        }
        return result
    }


    /// `GET /api/mail/drafts` — newest first, sorted server-side by
    /// `updated_at`.
    func drafts() async throws -> [Wire.Draft] {
        let (data, response) = try await send("GET", "/api/mail/drafts", body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode([Wire.Draft].self, from: data)
        } catch {
            throw MailrsError.decoding("drafts — \(error)")
        }
    }


    /// `POST /api/mail/drafts` — returns the id, new or the one given.
    func saveDraft(_ draft: Wire.SaveDraftRequest) async throws -> Int64 {
        let (data, response) = try await send(
            "POST", "/api/mail/drafts", body: try JSONEncoder().encode(draft), authorized: true
        )
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode(Wire.SaveDraftResponse.self, from: data).id
        } catch {
            throw MailrsError.decoding("save draft — \(error)")
        }
    }


    /// `DELETE /api/mail/drafts/{id}`.
    func deleteDraft(id: Int64) async throws {
        try await verb("DELETE", "/api/mail/drafts/\(id)")
    }
}
