import Foundation
import Observation
import UniformTypeIdentifiers

@MainActor
@Observable
final class ComposeModel {
    enum Mode: Equatable, Sendable {
        case new
        case reply(threadId: String, messageId: String)
        case replyAll(threadId: String, messageId: String)
        case forward(threadId: String?, messageId: String, attachmentsFromUid: Int?)
    }

    struct Quoted: Equatable, Sendable {
        let sender: String
        let date: Date
        let text: String
        let html: String?
    }

    // MARK: state
    var mode: Mode = .new
    var toText: String = ""
    var ccText: String = ""
    var bccText: String = ""
    var subject: String = ""
    var body: String = ""
    var attachments: [LocalAttachment] = []
    var scheduledAt: Date?
    var showCc: Bool = false
    var showBcc: Bool = false
    var signature: String = ""
    var signatureEnabled: Bool = false
    var quoted: Quoted?
    private(set) var isSending: Bool = false
    var errorMessage: String?
    var successMessage: String?

    let fromAddress: String
    private let sendService: MailSendService
    private let draftService: DraftService

    init(fromAddress: String, sendService: MailSendService, draftService: DraftService) {
        self.fromAddress = fromAddress
        self.sendService = sendService
        self.draftService = draftService
    }

    // MARK: configure from reply/forward

    func prepareReply(to message: ThreadMessage, threadId: String, replyAll: Bool) {
        mode = replyAll
            ? .replyAll(threadId: threadId, messageId: message.message_id)
            : .reply(threadId: threadId, messageId: message.message_id)

        subject = message.subject.hasPrefix("Re:") ? message.subject : "Re: \(message.subject)"

        let senderEmail = message.sender.extractedEmail ?? message.sender
        toText = senderEmail

        if replyAll {
            let others = message.recipients
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty && $0.extractedEmail != fromAddress && $0.extractedEmail != senderEmail }
            ccText = others.joined(separator: ", ")
            showCc = !ccText.isEmpty
        }

        quoted = Quoted(
            sender: message.sender,
            date: message.internal_date,
            text: message.preferredBodyText,
            html: message.html_body
        )
    }

    func prepareForward(from message: ThreadMessage, threadId: String) {
        mode = .forward(
            threadId: threadId,
            messageId: message.message_id,
            attachmentsFromUid: message.attachments.isEmpty ? nil : message.uid
        )
        subject = message.subject.hasPrefix("Fwd:") ? message.subject : "Fwd: \(message.subject)"
        quoted = Quoted(
            sender: message.sender,
            date: message.internal_date,
            text: message.preferredBodyText,
            html: message.html_body
        )
    }

    // MARK: attachments

    func addAttachment(url: URL) throws {
        let data = try Data(contentsOf: url)
        let contentType = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType
            ?? "application/octet-stream"
        attachments.append(LocalAttachment(
            filename: url.lastPathComponent,
            contentType: contentType,
            size: data.count,
            data: data
        ))
    }

    func removeAttachment(_ attachment: LocalAttachment) {
        attachments.removeAll { $0.id == attachment.id }
    }

    // MARK: send / save / cancel

    func send() async -> Bool {
        guard !isSending else { return false }
        let recipients = parseAddresses(toText)
        guard !recipients.isEmpty else {
            errorMessage = "收件人不能为空"
            return false
        }

        isSending = true
        errorMessage = nil
        defer { isSending = false }

        let finalBody = composeFinalBody(plain: true)
        let finalHtml = composeFinalBody(plain: false)

        var payload = SendPayload(
            from: fromAddress,
            to: recipients,
            cc: parseAddresses(ccText),
            bcc: parseAddresses(bccText),
            subject: subject,
            body: finalBody,
            html_body: finalHtml
        )

        switch mode {
        case .new: break
        case .reply(_, let messageId), .replyAll(_, let messageId):
            payload.inReplyTo = messageId
        case .forward(_, let messageId, let attUid):
            payload.forwardMessageId = messageId
            payload.forwardAttachmentsFrom = attUid
        }

        payload.scheduledAt = scheduledAt
        payload.attachments = attachments.map {
            AttachmentUpload(filename: $0.filename, contentType: $0.contentType, data: $0.data)
        }

        do {
            let result = try await sendService.send(payload)
            if result.success {
                successMessage = scheduledAt != nil ? "已安排定时发送" : "已发送"
                return true
            } else {
                errorMessage = result.message ?? "发送失败"
                return false
            }
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            return false
        }
    }

    func saveDraft() async -> Bool {
        let req = SaveDraftRequest(
            to: toText.isEmpty ? nil : toText,
            cc: ccText.isEmpty ? nil : ccText,
            bcc: bccText.isEmpty ? nil : bccText,
            subject: subject.isEmpty ? nil : subject,
            body: body.isEmpty ? nil : body,
            reply_to_thread_id: {
                switch mode {
                case .reply(let tid, _), .replyAll(let tid, _): return tid
                case .forward(let tid, _, _): return tid
                case .new: return nil
                }
            }()
        )
        do {
            let result = try await draftService.save(req)
            if result.success { successMessage = "草稿已保存" }
            return result.success
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            return false
        }
    }

    // MARK: private

    private func composeFinalBody(plain: Bool) -> String {
        let sig = (signatureEnabled && !signature.isEmpty)
            ? "\n\n-- \n\(signature)"
            : ""

        if plain {
            var out = body + sig
            if let q = quoted {
                let header = "\n\nOn \(formattedDate(q.date)), \(q.sender) wrote:\n"
                let quotedText = q.text
                    .split(separator: "\n", omittingEmptySubsequences: false)
                    .map { "> \($0)" }
                    .joined(separator: "\n")
                out += header + quotedText
            }
            return out
        }

        // HTML version: user-typed plain text is escaped + <br>-ized.
        // Quoted text keeps its original HTML when available.
        let userHtml = (body + sig)
            .htmlEscaped
            .replacingOccurrences(of: "\n", with: "<br>")

        var out = userHtml
        if let q = quoted {
            let headerText = "\n\nOn \(formattedDate(q.date)), \(q.sender) wrote:\n"
            let headerHtml = headerText.htmlEscaped.replacingOccurrences(of: "\n", with: "<br>")
            let quotedInner: String
            if let html = q.html, !html.isEmpty {
                quotedInner = html
            } else {
                quotedInner = q.text.htmlEscaped.replacingOccurrences(of: "\n", with: "<br>")
            }
            out += headerHtml
            out += "<blockquote style=\"border-left: 2px solid #ccc; margin-left: 0; padding-left: 8px;\">\n"
            out += quotedInner
            out += "\n</blockquote>"
        }
        return out
    }

    private func formattedDate(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .short
        return f.string(from: date)
    }

    private func parseAddresses(_ raw: String) -> [String] {
        raw.split { $0 == "," || $0 == ";" || $0 == "\n" }
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }
}

private extension String {
    var htmlEscaped: String {
        self
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
    }
}

struct LocalAttachment: Identifiable, Hashable, Sendable {
    let id: UUID
    let filename: String
    let contentType: String
    let size: Int
    let data: Data

    init(filename: String, contentType: String, size: Int, data: Data) {
        self.id = UUID()
        self.filename = filename
        self.contentType = contentType
        self.size = size
        self.data = data
    }
}
