import Foundation

/// How long the server holds an immediate send so Undo can still stop it,
/// and how long the app offers the Undo. The same numbers as the web
/// (`UNDO_SECONDS` in `web/src/lib/send-mail.ts`, its toast 8 s): the offer
/// ends before the hold, so a tap near the end still lands in time.
enum SendHold {
    static let seconds = 10
    static let offered: Duration = .seconds(8)
}

/// The addresses a message can leave by: the signed-in one first, then
/// what the server lets this account send as (`send_as` on
/// `GET /api/auth/me`), without repeats — the web's `sendAddresses`.
enum SendAddresses {
    static func list(own: String, sendAs: [String]) -> [String] {
        var out: [String] = []
        for raw in [own] + sendAs {
            let address = raw.trimmingCharacters(in: .whitespaces)
            if address.isEmpty { continue }
            if out.contains(where: { $0.lowercased() == address.lowercased() }) { continue }
            out.append(address)
        }
        return out
    }

    /// The address a reply leaves by: the one of mine the original was
    /// sent to, so the answer arrives from the address the sender wrote
    /// to; otherwise the signed-in one.
    static func replyFrom(options: [String], recipients: String) -> String {
        let written = recipients.lowercased()
        for address in options where written.contains(address.lowercased()) {
            return address
        }
        return options.first ?? ""
    }
}

extension Wire {
    /// One saved message template.
    ///
    /// Backend: `crates/webapi/src/handlers/prefs.rs` — `list_templates`
    /// (`TemplateWire` in `crates/core-api/src/method/admin/userdata.rs`).
    struct Template: Decodable, Identifiable, Equatable, Sendable {
        let id: Int64
        let name: String
        let subject: String
        let textBody: String

        enum CodingKeys: String, CodingKey {
            case id
            case name
            case subject
            case textBody = "text_body"
        }
    }
}

/// What applying a template does to a composer, as the web does it: the
/// subject only where the message has none of its own yet (a reply keeps
/// "Re: …"), and the body replaced by the template's text.
enum TemplateApply {
    struct Result: Equatable {
        let subject: String
        let body: String
    }

    static func apply(_ template: Wire.Template, subject: String, body: String, isReply: Bool) -> Result {
        var newSubject = subject
        if !template.subject.isEmpty && !isReply { newSubject = template.subject }
        var newBody = body
        if !template.textBody.isEmpty { newBody = template.textBody }
        return Result(subject: newSubject, body: newBody)
    }
}

extension SendSchedule {
    /// The time a send goes with: a "Send later" choice wins; plain Send
    /// keeps a time the resumed draft carried, unless that time has passed.
    static func when(
        _ schedule: SendSchedule, kept: Int64?, now: Date = Date(), calendar: Calendar = .current
    ) -> Int64? {
        if schedule != .now { return schedule.fireDate(after: now, calendar: calendar) }
        guard let kept, kept > Int64(now.timeIntervalSince1970) else { return nil }
        return kept
    }
}

extension MailrsClient {
    /// `GET /api/mail/templates`.
    func templates() async throws -> [Wire.Template] {
        try await getJSON("/api/mail/templates")
    }

    /// `DELETE /api/mail/pending/{message_id}` — stop a send the server is
    /// still holding. 409 once it is on its way, which `verb` throws as
    /// `MailrsError.server(status: 409)`.
    func cancelPendingSend(messageId: String) async throws {
        try await verb("DELETE", "/api/mail/pending/\(MailrsClient.segment(messageId))")
    }
}
