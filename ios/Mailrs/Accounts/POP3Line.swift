import Foundation

/// Reading what a POP3 server says.
///
/// POP3 has no response codes and no tags: every reply is `+OK` or
/// `-ERR` and the rest of the line is free text. That makes two things
/// this client has to be careful about, and both are here rather than
/// in the socket.
enum POP3 {
    /// A reply, and what the server said about it.
    struct Reply: Equatable {
        let ok: Bool
        let text: String
    }

    static func reply(_ line: String) -> Reply? {
        let t = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.hasPrefix("+OK") {
            return Reply(ok: true, text: String(t.dropFirst(3)).trimmingCharacters(in: .whitespaces))
        }
        if t.hasPrefix("-ERR") {
            return Reply(
                ok: false, text: String(t.dropFirst(4)).trimmingCharacters(in: .whitespaces))
        }
        return nil
    }

    /// `UIDL` — the only durable identity POP3 offers.
    ///
    /// Message **numbers** are renumbered on every session: message 3
    /// today is a different message tomorrow. Anything that remembers
    /// what has been seen has to remember the uidl, and a client that
    /// remembers numbers re-downloads the mailbox after every delete
    /// somebody makes elsewhere.
    struct Uidl: Equatable {
        let number: Int
        let id: String
    }

    /// One line of a `UIDL` listing: `3 QhdPYR:00WBw1Ph7x7`.
    static func uidl(_ line: String) -> Uidl? {
        let parts = line.trimmingCharacters(in: .whitespacesAndNewlines)
            .split(separator: " ", maxSplits: 1)
        guard parts.count == 2, let n = Int(parts[0]) else { return nil }
        let id = parts[1].trimmingCharacters(in: .whitespaces)
        return id.isEmpty ? nil : Uidl(number: n, id: id)
    }

    /// Undo the dot-stuffing a server applies to a retrieved message.
    ///
    /// The mirror of what SMTP does on the way out: a body line that
    /// began with `.` arrives doubled, and a client that does not undo
    /// it corrupts every message containing such a line. `.` alone on
    /// a line ends the response and is not part of the message.
    static func unstuffed(_ lines: [String]) -> String {
        lines
            .prefix { $0 != "." }
            .map { $0.hasPrefix("..") ? String($0.dropFirst()) : $0 }
            .joined(separator: "\r\n")
    }

    /// Whether a refusal means the credential is wrong.
    ///
    /// POP3 has no code for it, so the words are all there is. One is
    /// a button to press, the other is waiting.
    static func isAuthenticationFailure(_ text: String) -> Bool {
        let t = text.uppercased()
        return t.contains("AUTHENTICATION FAILED")
            || t.contains("INVALID") && t.contains("PASSWORD")
            || t.contains("LOGIN FAILED")
            || t.contains("AUTH")  && t.contains("FAIL")
    }
}
