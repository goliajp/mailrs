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
        attachments: [MultipartForm.FilePart] = [],
        from: String = ""
    ) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await sendWithFeedback {
            if attachments.isEmpty {
                return try await client.sendReply(
                    to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                    inReplyTo: inReplyTo, threadId: threadId, from: from
                )
            }
            // The multipart route, with both threading fields riding
            // along — a reply that lost them arrives detached.
            return try await client.sendMultipart(
                to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                attachments: attachments,
                inReplyTo: inReplyTo, replyToThreadId: threadId, from: from
            )
        }
    }


    func sendForward(
        to recipients: [String], cc: [String] = [], bcc: [String] = [],
        subject: String, body: String,
        forwardMessageId: String, forwardAttachmentsFrom: UInt32?,
        attachments: [MultipartForm.FilePart] = [],
        from: String = ""
    ) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await sendWithFeedback {
            if attachments.isEmpty {
                return try await client.sendForward(
                    to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                    forwardMessageId: forwardMessageId,
                    forwardAttachmentsFrom: forwardAttachmentsFrom, from: from
                )
            }
            // The server appends the original and EXTENDS the file
            // list (inline_forward_content), so the added files and
            // the forwarded ones coexist.
            return try await client.sendMultipart(
                to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                attachments: attachments,
                forwardMessageId: forwardMessageId,
                forwardAttachmentsFrom: forwardAttachmentsFrom, from: from
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
        attachments: [MultipartForm.FilePart] = [],
        scheduledAt: Int64? = nil,
        redraftOf: String? = nil,
        redraftKeep: [Int]? = nil,
        from: String = ""
    ) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await sendWithFeedback {
            // A re-edit goes the multipart way even with no file of its
            // own: `redraft_keep` is a form field, and the JSON route
            // has nowhere to put it — sent without it the server keeps
            // every carried attachment, including the ones just
            // removed.
            if attachments.isEmpty && redraftOf == nil {
                return try await client.sendNew(
                    to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                    scheduledAt: scheduledAt, from: from
                )
            }
            // Files ride the multipart route; the JSON route has no
            // field for them.
            return try await client.sendMultipart(
                to: recipients, cc: cc, bcc: bcc, subject: subject, body: body,
                attachments: attachments, scheduledAt: scheduledAt,
                redraftOf: redraftOf, redraftKeep: redraftKeep, from: from
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


    /// Send it again, byte for byte.
    ///
    /// Only offered where `can_resend` is set — the server reads an
    /// empty envelope reference as "the bytes are not on disk" and
    /// answers 409. The list is re-read afterwards rather than
    /// adjusted here: a resend makes a *new* row with its own status,
    /// and guessing at that shape would put a line on screen the
    /// server never agreed to.
    func resend(_ row: SendJoin.Row) async {
        guard let client, let sendId = row.sendId else { return }
        do {
            try await client.resend(sendId: sendId)
            await loadSendRows()
        } catch {
            banner = error.localizedDescription
        }
    }


    /// The fields of a sent message, to edit before it goes again.
    ///
    /// The half that fixes anything: a resend re-enqueues the stored
    /// bytes unchanged, so a message that failed because the address
    /// was wrong fails again.
    func redraft(_ row: SendJoin.Row) async -> Wire.Redraft? {
        guard let client, let sendId = row.sendId else { return nil }
        do {
            return try await client.redraft(sendId: sendId)
        } catch {
            banner = error.localizedDescription
            return nil
        }
    }


    /// The bytes a send actually put on the wire.
    func sendSource(_ row: SendJoin.Row) async -> String? {
        guard let client, let sendId = row.sendId else { return nil }
        do {
            return try await client.sendSource(sendId: sendId)
        } catch {
            banner = error.localizedDescription
            return nil
        }
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


/// Mail that has not left yet.
@MainActor
extension Session {
    func loadScheduled() async {
        guard let client else { return }
        guard let items = try? await client.scheduledSends() else { return }
        scheduledSends = items
    }

    /// Stop one before it goes.
    ///
    /// Off the list first, then the wire — and back if the wire
    /// refuses. A row that lingers after Cancel reads as a message
    /// that is still going out, which is the one thing this screen
    /// exists to answer.
    /// Move one to a different time.
    ///
    /// The list is re-read rather than patched: the server sorts by
    /// when, so a row whose time changed belongs somewhere else, and
    /// a locally-edited row would sit in the old position until the
    /// next refresh.
    func rescheduleScheduled(_ send: Wire.ScheduledSend, to when: Int64) async {
        guard let client else { return }
        do {
            try await client.rescheduleScheduled(id: send.id, to: when)
            await loadScheduled()
        } catch {
            banner = error.localizedDescription
        }
    }

    func cancelScheduled(_ send: Wire.ScheduledSend) async {
        guard let client else { return }
        let previous = scheduledSends
        withAnimation { scheduledSends.removeAll { $0.id == send.id } }
        do {
            try await client.cancelScheduled(id: send.id)
        } catch {
            scheduledSends = previous
            banner = error.localizedDescription
        }
    }
}
