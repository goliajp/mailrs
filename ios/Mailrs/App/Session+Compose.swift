import Foundation
import SwiftUI

/// Writing and sending, and the drafts that outlive a sheet.
///
/// Split out of `Session.swift` at the 500-line limit this
/// repository holds every language to — and which did not look at
/// `ios/` until now. One type, several subjects.
@MainActor
extension Session {

    func sendReply(
        to recipients: [String], cc: [String] = [], bcc: [String] = [],
        subject: String, body: String,
        inReplyTo: String?, threadId: String,
        attachments: [MultipartForm.FilePart] = []
    ) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await sendWithFeedback {
            if attachments.isEmpty {
                return try await client.sendReply(
                    to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                    inReplyTo: inReplyTo, threadId: threadId
                )
            }
            // The multipart route, with both threading fields riding
            // along — a reply that lost them arrives detached.
            return try await client.sendMultipart(
                to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                attachments: attachments,
                inReplyTo: inReplyTo, replyToThreadId: threadId
            )
        }
    }


    func sendForward(
        to recipients: [String], cc: [String] = [], bcc: [String] = [],
        subject: String, body: String,
        forwardMessageId: String, forwardAttachmentsFrom: UInt32?,
        attachments: [MultipartForm.FilePart] = []
    ) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await sendWithFeedback {
            if attachments.isEmpty {
                return try await client.sendForward(
                    to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                    forwardMessageId: forwardMessageId,
                    forwardAttachmentsFrom: forwardAttachmentsFrom
                )
            }
            // The server appends the original and EXTENDS the file
            // list (inline_forward_content), so the added files and
            // the forwarded ones coexist.
            return try await client.sendMultipart(
                to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                attachments: attachments,
                forwardMessageId: forwardMessageId,
                forwardAttachmentsFrom: forwardAttachmentsFrom
            )
        }
    }


    /// Send a message that is not a reply.
    ///
    /// Both threading fields stay nil. Sending `reply_to_thread_id` here
    /// would file a new message inside an existing conversation, which
    /// is the mirror of the bug that made replies arrive unthreaded.
    func sendNew(
        to recipients: [String], cc: [String] = [], bcc: [String] = [],
        subject: String, body: String,
        attachments: [MultipartForm.FilePart] = []
    ) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await sendWithFeedback {
            if attachments.isEmpty {
                return try await client.sendNew(
                    to: recipients, cc: cc, bcc: bcc, subject: subject, body: body
                )
            }
            // Files ride the multipart route; the JSON route has no
            // field for them.
            return try await client.sendMultipart(
                to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                attachments: attachments
            )
        }
    }


    /// The physical answer to Send. The sheet dismissing says it too,
    /// but the thumb is on the button and the eyes may not be — Gmail
    /// and Apple Mail both confirm a send through the hand. Failure taps
    /// differently *before* the error text appears, for the same reason.
    private func sendWithFeedback(_ send: () async throws -> some Sendable) async throws {
        do {
            _ = try await send()
            UINotificationFeedbackGenerator().notificationOccurred(.success)
        } catch {
            UINotificationFeedbackGenerator().notificationOccurred(.error)
            throw error
        }
    }


    func loadDrafts() async {
        guard let client else { return }
        do {
            drafts = try await client.drafts()
            draftsFailure = nil
        } catch {
            draftsFailure = error.localizedDescription
        }
    }


    /// Save a compose session, returning the id the server gave it.
    ///
    /// The caller keeps that id and passes it back on the next save, so
    /// one session upserts one draft. Posting without it on every
    /// autosave would leave a new draft per tick.
    func saveDraft(
        id: Int64?, to: String, cc: String = "", bcc: String = "",
        subject: String, body: String, replyToThreadId: String?
    ) async -> Int64? {
        guard let client else { return nil }
        let request = Wire.SaveDraftRequest(
            id: id, to: to, cc: cc, bcc: bcc, subject: subject, body: body,
            replyToThreadId: replyToThreadId
        )
        return try? await client.saveDraft(request)
    }


    func deleteDraft(id: Int64) async {
        guard let client else { return }
        try? await client.deleteDraft(id: id)
        drafts.removeAll { $0.id == id }
    }


    /// The Send list's rows: both endpoints, joined. Failures are the
    /// list's whole reason to exist, so a partial fetch does not render
    /// as an empty "Nothing sent yet".
    func loadSendRows() async {
        guard let client else { return }
        if sendRows.isEmpty { initialLoading = true }
        do {
            async let messages = client.sentMessages()
            async let sends = client.sends()
            let rows = SendJoin.join(messages: try await messages, sends: try await sends)
            withAnimation { sendRows = rows }
        } catch {
            banner = error.localizedDescription
        }
        initialLoading = false
    }
}


/// The signature, from the account rather than from the device.
@MainActor
extension Session {
    /// Read it at sign-in. The default one, or the first if the server
    /// has several and marked none — a phone edits one signature, and
    /// picking nothing would mean signing with nothing.
    func loadSignature() async {
        guard let client else { return }
        guard let list = try? await client.signatures() else { return }
        var chosen = list.first(where: \.isDefault)
        if chosen == nil { chosen = list.first }
        guard let chosen else { return }
        signature = chosen.textContent
        signatureId = chosen.id
    }

    /// Save what was typed, dropping the row it replaces.
    ///
    /// Clearing the text deletes the signature outright rather than
    /// storing an empty one — "no signature" is the absence of a row,
    /// not a row that says nothing.
    func saveSignature(_ text: String) async {
        guard let client else { return }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            if trimmed.isEmpty {
                if let signatureId { try await client.deleteSignature(id: signatureId) }
                signature = ""
                signatureId = nil
                return
            }
            signatureId = try await client.replaceDefaultSignature(
                text: trimmed, replacing: signatureId)
            signature = trimmed
        } catch {
            banner = error.localizedDescription
        }
    }
}
