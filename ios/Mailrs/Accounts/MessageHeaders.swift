import Foundation

/// The few headers a list row needs, out of a raw message.
///
/// Not a MIME parser: this reads what a row shows — who it is from,
/// what it is about, when it arrived, and the identity that threads
/// it. The body is somebody else's problem.
enum MessageHeaders {
    struct Parsed: Equatable {
        var messageId = ""
        var from = ""
        var subject = ""
        var date = ""
        var inReplyTo = ""
        /// Where a reply goes when it is not back to the sender — a
        /// mailing list, or a no-reply address that names a real one.
        var replyTo = ""
        /// The conversation so far, oldest first. A reply that drops it
        /// starts a new conversation in every client that reads it.
        var references: [String] = []
        /// Who it was addressed to, verbatim — a reply-all needs them.
        var to = ""
        var cc = ""
    }

    /// Read the header block.
    ///
    /// Stops at the blank line that ends it — a body may contain
    /// anything, including lines that look exactly like headers, and a
    /// parser that keeps going reads them.
    static func parse(_ raw: String) -> Parsed {
        var out = Parsed()
        for line in unfolded(raw) {
            guard let colon = line.firstIndex(of: ":") else { continue }
            let name = line[..<colon].lowercased()
            let value = line[line.index(after: colon)...]
                .trimmingCharacters(in: .whitespaces)
            switch name {
            case "message-id": if out.messageId.isEmpty { out.messageId = value }
            case "from": if out.from.isEmpty { out.from = EncodedWord.decode(value) }
            // Decoded here rather than at the call site: every
            // reader of a subject wants the text, and one that forgets
            // shows `=?UTF-8?B?` to somebody.
            case "subject": if out.subject.isEmpty { out.subject = EncodedWord.decode(value) }
            case "date": if out.date.isEmpty { out.date = value }
            case "in-reply-to": if out.inReplyTo.isEmpty { out.inReplyTo = value }
            case "reply-to": if out.replyTo.isEmpty { out.replyTo = EncodedWord.decode(value) }
            case "to": if out.to.isEmpty { out.to = EncodedWord.decode(value) }
            case "cc": if out.cc.isEmpty { out.cc = EncodedWord.decode(value) }
            case "references":
                if out.references.isEmpty {
                    // Whitespace-separated, and folded over several
                    // lines on any conversation of length — which is
                    // why this reads unfolded headers rather than
                    // lines.
                    out.references = value.split(whereSeparator: { $0.isWhitespace })
                        .map(String.init)
                }
            default: break
            }
        }
        return out
    }

    /// The header block, one logical header per element.
    ///
    /// **Folding is the trap.** RFC 5322 lets a header continue on the
    /// next line when it starts with a space or a tab, and a long
    /// Subject usually does. A parser that reads lines rather than
    /// headers gets half a subject — and, worse, may read the
    /// continuation as a header of its own.
    static func unfolded(_ raw: String) -> [String] {
        var out: [String] = []
        for line in raw.replacingOccurrences(of: "\r\n", with: "\n").split(
            separator: "\n", omittingEmptySubsequences: false)
        {
            if line.isEmpty { break }  // the blank line ends the block
            if line.hasPrefix(" ") || line.hasPrefix("\t"), !out.isEmpty {
                out[out.count - 1] += " " + line.trimmingCharacters(in: .whitespaces)
            } else {
                out.append(String(line))
            }
        }
        return out
    }

    /// The display name from a `From`, or the address.
    ///
    /// `Alice Smith <alice@example.com>` → `Alice Smith`; a bare
    /// address is its own name. Quotes come off, because a name with a
    /// comma in it is quoted and the quotes are syntax.
    static func senderName(_ from: String) -> String {
        let t = from.trimmingCharacters(in: .whitespaces)
        guard let open = t.lastIndex(of: "<") else { return t }
        let name = t[..<open].trimmingCharacters(in: .whitespaces)
        if name.isEmpty {
            // `<alice@example.com>` with no name: the address, without
            // the brackets that are syntax.
            if let close = t.lastIndex(of: ">"), open < close {
                return String(t[t.index(after: open)..<close])
            }
            return t
        }
        if name.hasPrefix("\""), name.hasSuffix("\""), name.count >= 2 {
            return String(name.dropFirst().dropLast())
                .replacingOccurrences(of: "\\\"", with: "\"")
        }
        return name
    }
}
