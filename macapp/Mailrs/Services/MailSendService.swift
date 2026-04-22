import Foundation

struct AttachmentUpload: Sendable {
    let filename: String
    let contentType: String
    let data: Data
}

struct SendResult: Decodable, Sendable {
    let success: Bool
    let message: String?
    let message_id: String?
}

struct SendPayload: Sendable {
    var from: String
    var to: [String]
    var cc: [String] = []
    var bcc: [String] = []
    var subject: String
    var body: String       // plain text
    var html_body: String  // html
    var inReplyTo: String?
    var forwardMessageId: String?
    var forwardAttachmentsFrom: Int?
    var scheduledAt: Date?
    var attachments: [AttachmentUpload] = []

    var hasAttachments: Bool { !attachments.isEmpty }
}

struct MailSendService {
    let api: ApiClient

    func send(_ payload: SendPayload) async throws -> SendResult {
        if payload.hasAttachments {
            return try await sendMultipart(payload)
        }
        return try await sendJSON(payload)
    }

    // MARK: private

    private func sendJSON(_ p: SendPayload) async throws -> SendResult {
        struct Body: Encodable {
            let from: String
            let to: [String]
            let cc: [String]
            let bcc: [String]
            let subject: String
            let body: String
            let html_body: String
            let in_reply_to: String?
            let forward_message_id: String?
            let forward_attachments_from: Int?
            let scheduled_at: String?
        }
        let iso = ISO8601DateFormatter()
        let req = Body(
            from: p.from, to: p.to, cc: p.cc, bcc: p.bcc,
            subject: p.subject, body: p.body, html_body: p.html_body,
            in_reply_to: p.inReplyTo,
            forward_message_id: p.forwardMessageId,
            forward_attachments_from: p.forwardAttachmentsFrom,
            scheduled_at: p.scheduledAt.map { iso.string(from: $0) }
        )
        return try await api.post("/api/mail/send", body: req)
    }

    private func sendMultipart(_ p: SendPayload) async throws -> SendResult {
        var m = MultipartBuilder()
        m.appendText(name: "from", value: p.from)
        m.appendText(name: "subject", value: p.subject)
        m.appendText(name: "body", value: p.body)
        m.appendText(name: "html_body", value: p.html_body)

        for r in p.to { m.appendText(name: "to", value: r) }
        for r in p.cc { m.appendText(name: "cc", value: r) }
        for r in p.bcc { m.appendText(name: "bcc", value: r) }

        if let inReplyTo = p.inReplyTo { m.appendText(name: "in_reply_to", value: inReplyTo) }
        if let fmid = p.forwardMessageId { m.appendText(name: "forward_message_id", value: fmid) }
        if let fuid = p.forwardAttachmentsFrom { m.appendText(name: "forward_attachments_from", value: String(fuid)) }
        if let scheduled = p.scheduledAt {
            m.appendText(name: "scheduled_at", value: ISO8601DateFormatter().string(from: scheduled))
        }
        for att in p.attachments {
            m.appendFile(name: "attachments", filename: att.filename,
                         contentType: att.contentType, data: att.data)
        }

        let bodyData = m.finalize()
        return try await api.postMultipart(
            "/api/mail/send-multipart",
            contentType: m.contentTypeHeader,
            body: bodyData
        )
    }
}
