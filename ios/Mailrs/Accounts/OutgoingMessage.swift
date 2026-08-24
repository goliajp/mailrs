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
            // **The envelope is a command line.** An address with a
            // control character in it does not become a worse address
            // — it becomes another SMTP command, and the message goes
            // somewhere the sender never typed. Dropped rather than
            // cleaned: an address that arrived with a line break in it
            // was not a typo.
            let trimmed = address.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else { continue }
            guard !trimmed.unicodeScalars.contains(where: {
                $0.value < 0x20 || $0.value == 0x7F
            }) else { continue }
            // A duplicate recipient is a duplicate delivery: the same
            // person on To and Cc gets the message twice.
            guard seen.insert(trimmed.lowercased()).inserted else { continue }
            out.append(trimmed)
        }
        return out
    }

    /// The message itself.
    /// Everything down to the blank line, and the blank line.
    ///
    /// One builder, used by both `text` and `pieces`. Two of them is
    /// two answers to the same question, and the one nobody reads is
    /// the one that drifts.
    private static func headerBlock(
        _ draft: Draft, id: String, date: Date, timeZone: TimeZone
    ) -> String {
        var lines: [String] = []
        lines.append("Message-ID: <\(id)>")
        lines.append("Date: \(MailDate.rfc5322(date, timeZone: timeZone))")
        lines.append("From: \(headerValue(address(draft.fromName, draft.from)))")
        if !draft.to.isEmpty {
            lines.append("To: \(headerValue(draft.to.joined(separator: ", ")))")
        }
        if !draft.cc.isEmpty {
            lines.append("Cc: \(headerValue(draft.cc.joined(separator: ", ")))")
        }
        // Never a Bcc header. It is in the envelope, and writing it
        // here is how a blind copy stops being blind.
        lines.append("Subject: \(headerValue(EncodedWord.encode(draft.subject)))")
        if !draft.inReplyTo.isEmpty {
            lines.append("In-Reply-To: \(headerValue(draft.inReplyTo))")
            var chain = draft.references
            if !chain.contains(draft.inReplyTo) { chain.append(draft.inReplyTo) }
            lines.append("References: \(headerValue(chain.joined(separator: " ")))")
        }
        lines.append("MIME-Version: 1.0")
        if draft.attachments.isEmpty {
            lines.append("Content-Type: text/plain; charset=utf-8")
            // 8bit, not base64: the body stays readable in every tool
            // that looks at a raw message, including the person
            // debugging this.
            lines.append("Content-Transfer-Encoding: 8bit")
        } else {
            lines.append(
                "Content-Type: multipart/mixed; boundary=\"\(boundary(draft, id))\"")
        }
        lines.append("")
        return lines.joined(separator: "\r\n") + "\r\n"
    }

    /// The message, in pieces small enough to hand to a socket.
    ///
    /// The same bytes `text` produces — asserted, because two builders
    /// that disagree is the defect this exists to avoid — but never
    /// all of it at once. An attachment is encoded 57 raw bytes at a
    /// time, which is exactly one 76-character base64 line, so what is
    /// held while sending a 25 MB file is 57 bytes and not 25 MB.
    ///
    /// Lazy, so nothing is computed until the socket asks for it: a
    /// send that fails at `RCPT TO` never encodes anything.
    static func pieces(
        _ draft: Draft, id: String, date: Date, timeZone: TimeZone = .current
    ) -> AnySequence<String> {
        AnySequence { () -> AnyIterator<String> in
            var stage = 0
            var attachmentIndex = 0
            var offset = 0
            var wroteAttachmentHeader = false
            let boundaryText = boundary(draft, id)
            return AnyIterator {
                switch stage {
                case 0:
                    stage = draft.attachments.isEmpty ? 1 : 2
                    return headerBlock(draft, id: id, date: date, timeZone: timeZone)
                case 1:
                    stage = 99
                    return normalised(draft.body)
                case 2:
                    stage = 3
                    // The text first, always. Every reader shows the
                    // first text part it finds, and a message whose
                    // first part is a PDF opens as a PDF with the words
                    // underneath it.
                    return "--\(boundaryText)\r\n"
                        + "Content-Type: text/plain; charset=utf-8\r\n"
                        + "Content-Transfer-Encoding: 8bit\r\n\r\n"
                        + normalised(draft.body)
                case 3:
                    // A loop rather than a recursive call: an
                    // attachment can be empty, and "hand back the next
                    // thing" then has to skip it without a stack frame
                    // that has nothing to name.
                    while attachmentIndex < draft.attachments.count {
                        let attachment = draft.attachments[attachmentIndex]
                        if !wroteAttachmentHeader {
                            wroteAttachmentHeader = true
                            let name = headerSafe(attachment.filename)
                            // The name in both places: `Content-Type:
                            // name=` is the older spelling and some
                            // readers still look only there.
                            return "--\(boundaryText)\r\n"
                                + "Content-Type: \(attachment.mimeType); name=\"\(name)\"\r\n"
                                + "Content-Disposition: attachment; filename=\"\(name)\"\r\n"
                                + "Content-Transfer-Encoding: base64\r\n\r\n"
                        }
                        if offset >= attachment.bytes.count {
                            attachmentIndex += 1
                            offset = 0
                            wroteAttachmentHeader = false
                            continue
                        }
                        // 57 bytes in, 76 base64 characters out — the
                        // line length RFC 2045 asks for, and the reason
                        // the number is 57 and not something rounder.
                        let from = attachment.bytes.index(
                            attachment.bytes.startIndex, offsetBy: offset)
                        let to = attachment.bytes.index(
                            from, offsetBy: min(57, attachment.bytes.count - offset))
                        offset += attachment.bytes.distance(from: from, to: to)
                        return Data(attachment.bytes[from..<to]).base64EncodedString() + "\r\n"
                    }
                    stage = 99
                    return "--\(boundaryText)--\r\n"
                default:
                    return nil
                }
            }
        }
    }

    /// The message itself, whole.
    ///
    /// `pieces` is the one that streams; this is here for callers that
    /// want the bytes in hand — the tests that read a built message
    /// back with this app's own parser, mostly — and is asserted to
    /// produce exactly what the pieces concatenate to.
    static func text(_ draft: Draft, id: String, date: Date, timeZone: TimeZone = .current)
        -> String
    {
        pieces(draft, id: id, date: date, timeZone: timeZone).joined()
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
    /// Any value on its way into a header.
    ///
    /// Belt as well as braces: `EncodedWord.decode` already strips
    /// these at the boundary where a stranger's bytes become text, and
    /// this is the place that would emit the broken header if anything
    /// ever got past it. A header value cannot contain a control
    /// character — that is what ends a header — so nothing is lost by
    /// saying so twice.
    static func headerValue(_ text: String) -> String {
        String(
            String.UnicodeScalarView(
                text.unicodeScalars.filter { $0.value >= 0x20 && $0.value != 0x7F }))
    }

    private static func headerSafe(_ name: String) -> String {
        // **By scalar, not by character.** In Swift a CRLF is one
        // `Character` — a grapheme cluster — so filtering Characters
        // against `"\r"` and `"\n"` matches neither and the line break
        // survives. The identical code in Kotlin is correct, because a
        // Kotlin `Char` is a UTF-16 unit; here it let a filename inject
        // a header, and the assertion that caught it was written for
        // exactly that.
        String(
            String.UnicodeScalarView(
                name.unicodeScalars.filter {
                    $0 != "\"" && $0 != "\\" && $0.value >= 0x20 && $0.value != 0x7F
                }))
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
