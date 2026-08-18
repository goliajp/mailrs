import Foundation

/// The Send list's rows: sent mail joined with its delivery status.
///
/// Ported from `web/src/components/send-list/send-model.ts`, which is
/// where the semantics were paid for. Two sources contribute rows,
/// deduped on Message-ID: the sent axis (`/api/mail/sent`) holds the
/// messages the maildir sweep has filed, and the Send projection
/// (`/api/mail/sends`) holds delivery status. Either can be missing the
/// other's rows — a send that just left has no maildir copy yet, and
/// most old mail predates the projection entirely.
enum SendJoin {
    struct Row: Identifiable, Sendable {
        let threadId: String
        let uid: UInt32?
        let subject: String
        let to: String
        let date: Int64
        /// `nil` for mail that predates the projection. Absence says
        /// nothing rather than claiming delivery — the honest rendering
        /// the web view settled on.
        let status: String?
        /// The projection's own id, needed to ask for a resend — and
        /// `nil` for a row only the maildir knows about, which is
        /// exactly the mail the server cannot send again.
        let sendId: String?
        let canResend: Bool

        var id: String { key }
        let key: String
    }

    /// Normalise a Message-ID for comparison: no brackets, no case.
    ///
    /// Both sides store it bare, but if either ever gains brackets the
    /// join fails *silently* — every row simply loses its status — which
    /// is why normalisation is unconditional rather than trusted.
    static func joinKey(_ raw: String) -> String {
        var trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.hasPrefix("<") { trimmed.removeFirst() }
        if trimmed.hasSuffix(">") { trimmed.removeLast() }
        return trimmed.lowercased()
    }

    static func join(messages: [Wire.SentMessage], sends: [Wire.Send]) -> [Row] {
        // Index the projection by the *original* message where a resend
        // chain exists — `resent_from` points at it — keeping the newest
        // attempt, whose status is the one that matters.
        var byMessage: [String: Wire.Send] = [:]
        for send in sends {
            let key = joinKey(send.resentFrom ?? send.sendId)
            if key.isEmpty { continue }
            if let held = byMessage[key], held.createdAt > send.createdAt { continue }
            byMessage[key] = send
        }

        var rows: [String: Row] = [:]
        for message in messages {
            let key = joinKey(message.messageId)
            if key.isEmpty { continue }
            let send = byMessage[key]
            rows[key] = Row(
                threadId: message.threadId,
                uid: message.uid,
                subject: message.subject,
                to: message.to,
                date: message.internalDate,
                status: send?.status,
                sendId: send?.sendId,
                canResend: send?.canResend ?? false,
                key: key
            )
        }

        // Sends the sweep has not filed yet. Without this pass a send
        // that succeeded — accepted by the remote, row written — is
        // absent from the only screen that would show it.
        for (key, send) in byMessage where rows[key] == nil {
            rows[key] = Row(
                threadId: send.threadId,
                uid: nil,
                subject: send.subject,
                to: send.to.joined(separator: ", "),
                date: send.createdAt,
                status: send.status,
                sendId: send.sendId,
                canResend: send.canResend,
                key: key
            )
        }

        return rows.values.sorted { $0.date > $1.date }
    }
}
