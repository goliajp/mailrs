import Foundation

/// The few lines at the bottom that say who sent this.
///
/// Mail written on the phone left without one. The web has a signature
/// but keeps it in `localStorage`, so it is a property of a browser
/// rather than of a person — set it at the desk and mail from the
/// laptop, the phone and a second browser each sign differently, or not
/// at all. The server has had a per-user signature store the whole
/// time and nothing reads it; this client does, which makes the
/// signature follow the account.
enum MailSignature {
    /// `-- ` on a line of its own: RFC 3676 §4.3's separator, and what
    /// every reader keys on to fold a signature away or to strip it
    /// from a quote. The trailing space is part of it — without it the
    /// line is just two hyphens and nothing recognises it.
    static let separator = "-- "

    /// The body as it goes on the wire.
    ///
    /// An empty signature returns the body untouched rather than a
    /// separator with nothing beneath it. A body that already carries
    /// one is left alone: replies quote the original beneath what was
    /// typed, and a second signature between the two reads as though
    /// the sender signed the other person's message.
    static func append(body: String, signature: String) -> String {
        let sig = signature.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !sig.isEmpty else { return body }
        guard !carriesOne(body) else { return body }
        let text = body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return "\(separator)\n\(sig)" }
        return "\(text)\n\n\(separator)\n\(sig)"
    }

    /// Whether the text already has a separator line of its own.
    ///
    /// `isNewline`, not `split(separator: "\n")`: in Swift a CRLF is a
    /// single `Character` and is not equal to `"\n"`, so splitting on
    /// the literal leaves a message written by a Windows client as one
    /// long line and finds no separator anywhere in it.
    static func carriesOne(_ body: String) -> Bool {
        for line in body.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline) {
            let bare = line.trimmingCharacters(in: .whitespaces)
            if bare == "--" { return true }
        }
        return false
    }
}
