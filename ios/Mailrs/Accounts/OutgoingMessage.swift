import Foundation

/// An RFC 5322 message, ready to hand to a server.
///
/// Pure: what goes on the wire is decided here and tested here, and
/// `SMTPSession` only carries it. A message that is wrong is wrong
/// whether or not there is a server to refuse it, and most servers do
/// not refuse — they deliver it looking broken.
enum OutgoingMessage {
    struct Draft: Equatable {
        var from: String
        var fromName: String = ""
        var to: [String]
        var cc: [String] = []
        var subject: String = ""
        var body: String = ""
        var inReplyTo: String = ""
        /// The identities of the messages before this one, oldest
        /// first. Without it a reply starts a new conversation in every
        /// client that reads it, which is every client.
        var references: [String] = []
        /// What to send with it.
        var attachments: [Attachment] = []
    }

    /// A file on its way out.
    struct Attachment: Equatable, Identifiable {
        var filename: String
        var mimeType: String
        var bytes: Data
        /// Unique within one draft: two files may share a name.
        var id: String { "\(filename)-\(bytes.count)" }
    }

    /// Everyone the message goes to.
    ///
    /// The envelope, not the headers: `Bcc` recipients belong here and
    /// nowhere else, and a `Cc` recipient who is not in the envelope
    /// never receives it however clearly the header names them.
    static func envelope(_ draft: Draft, bcc: [String] = []) -> [String] {
        var seen = Set<String>()
        var out: [String] = []
        for address in draft.to + draft.cc + bcc {
            let trimmed = address.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else { continue }
            // A duplicate recipient is a duplicate delivery: the same
            // person on To and Cc gets the message twice.
            guard seen.insert(trimmed.lowercased()).inserted else { continue }
            out.append(trimmed)
        }
        return out
    }

    /// The message itself.
    static func text(_ draft: Draft, id: String, date: Date, timeZone: TimeZone = .current)
        -> String
    {
        var lines: [String] = []
        lines.append("Message-ID: <\(id)>")
        lines.append("Date: \(MailDate.rfc5322(date, timeZone: timeZone))")
        lines.append("From: \(address(draft.fromName, draft.from))")
        if !draft.to.isEmpty { lines.append("To: \(draft.to.joined(separator: ", "))") }
        if !draft.cc.isEmpty { lines.append("Cc: \(draft.cc.joined(separator: ", "))") }
        // Never a Bcc header. It is in the envelope, and writing it
        // here is how a blind copy stops being blind.
        lines.append("Subject: \(EncodedWord.encode(draft.subject))")
        if !draft.inReplyTo.isEmpty {
            lines.append("In-Reply-To: \(draft.inReplyTo)")
            var chain = draft.references
            if !chain.contains(draft.inReplyTo) { chain.append(draft.inReplyTo) }
            lines.append("References: \(chain.joined(separator: " "))")
        }
        lines.append("MIME-Version: 1.0")
        if draft.attachments.isEmpty {
            lines.append("Content-Type: text/plain; charset=utf-8")
            // 8bit, not base64: the body stays readable in every tool
            // that looks at a raw message, including the person
            // debugging this.
            lines.append("Content-Transfer-Encoding: 8bit")
            lines.append("")
            return lines.joined(separator: "\r\n") + "\r\n" + normalised(draft.body)
        }
        let boundary = boundary(draft, id)
        lines.append("Content-Type: multipart/mixed; boundary=\"\(boundary)\"")
        lines.append("")
        var parts = ""
        // The text first, always. Every reader shows the first text
        // part it finds, and a message whose first part is a PDF opens
        // as a PDF with the words underneath it.
        parts += "--\(boundary)\r\n"
        parts += "Content-Type: text/plain; charset=utf-8\r\n"
        parts += "Content-Transfer-Encoding: 8bit\r\n\r\n"
        parts += normalised(draft.body)
        for attachment in draft.attachments {
            let name = headerSafe(attachment.filename)
            parts += "--\(boundary)\r\n"
            // The name in both places: `Content-Type: name=` is the
            // older spelling and some readers still look only there.
            parts += "Content-Type: \(attachment.mimeType); name=\"\(name)\"\r\n"
            parts += "Content-Disposition: attachment; filename=\"\(name)\"\r\n"
            parts += "Content-Transfer-Encoding: base64\r\n\r\n"
            parts += wrapped(attachment.bytes.base64EncodedString())
        }
        parts += "--\(boundary)--\r\n"
        return lines.joined(separator: "\r\n") + "\r\n" + parts
    }

    /// A display name, quoted only when it has to be.
    ///
    /// `Ada Lovelace <a@b>` is fine; `Lovelace, Ada <a@b>` is two
    /// recipients to a parser that reads the comma, so a name with any
    /// of the specials gets quoted.
    static func address(_ name: String, _ email: String) -> String {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return email }
        let encoded = EncodedWord.encode(trimmed)
        // An encoded word is already safe, and quoting one stops it
        // being decoded at all.
        if encoded != trimmed { return "\(encoded) <\(email)>" }
        let specials = CharacterSet(charactersIn: "()<>[]:;@\\,.\"")
        if trimmed.rangeOfCharacter(from: specials) != nil {
            let escaped = trimmed.replacingOccurrences(of: "\\", with: "\\\\")
                .replacingOccurrences(of: "\"", with: "\\\"")
            return "\"\(escaped)\" <\(email)>"
        }
        return "\(trimmed) <\(email)>"
    }

    /// A boundary that cannot appear in the message.
    ///
    /// Derived from the message id, which is already unique — a
    /// boundary that turns up inside a part cuts the message in half
    /// at that point.
    private static func boundary(_ draft: Draft, _ id: String) -> String {
        let cleaned = id.filter { $0.isLetter || $0.isNumber }.prefix(24)
        return "----=_mailrs_\(cleaned)_\(draft.attachments.count)"
    }

    /// A filename that cannot break the header it sits in.
    ///
    /// Quotes and backslashes end the quoted string early, and a
    /// newline ends the header — which is how a filename becomes an
    /// injected header. RFC 2231 would encode a non-ASCII name; this
    /// keeps it as UTF-8, which every current reader accepts and which
    /// is what the alternative degrades to anyway.
    private static func headerSafe(_ name: String) -> String {
        String(name.filter { $0 != "\"" && $0 != "\\" && $0 != "\r" && $0 != "\n" })
    }

    /// Base64 at 76 characters, as RFC 2045 asks.
    private static func wrapped(_ text: String) -> String {
        var out: [String] = []
        var rest = Substring(text)
        while !rest.isEmpty {
            out.append(String(rest.prefix(76)))
            rest = rest.dropFirst(76)
        }
        return out.joined(separator: "\r\n") + "\r\n"
    }

    /// Every line ends CRLF, and no line is a bare dot.
    ///
    /// The dot-stuffing itself belongs to the session — it is a
    /// property of the DATA command, not of the message — but the line
    /// endings are the message's, and a body with bare newlines is
    /// what makes a message arrive as one long line.
    static func normalised(_ body: String) -> String {
        let unified = body.replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
        var text = unified.split(separator: "\n", omittingEmptySubsequences: false)
            .joined(separator: "\r\n")
        if !text.hasSuffix("\r\n") { text += "\r\n" }
        return text
    }
}
