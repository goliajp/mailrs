import Foundation

/// Dot-stuffing a message that arrives in pieces — RFC 5321 4.5.2.
///
/// The rule is simple: a line beginning with `.` gets another, because
/// a bare `.` on its own line is what ends the DATA block. Applied to
/// a whole message in memory it is a one-liner (`SMTP.dotStuffed` is
/// that one-liner).
///
/// **Applied to a stream it needs to remember where it is.** A chunk
/// can end exactly on a line break and the next begin with a dot, and
/// a stuffer that forgets treats that dot as mid-line text — which is
/// a message truncated at that point, arriving as a complete-looking
/// message that stops halfway. Every mail client has shipped that bug
/// once; a streaming one can ship it in a place a whole-message test
/// never reaches, because the boundary only exists when the message is
/// large enough to be split.
///
/// Not a formatter: this is a state machine with one bit of state, and
/// that bit is the whole of the difficulty.
final class DotStuffer {
    /// Whether the next character starts a line.
    ///
    /// True at the beginning, because a message whose very first line
    /// begins with a dot needs stuffing too.
    private var atLineStart = true

    /// Stuff one piece, remembering where it left off.
    func feed(_ chunk: String) -> String {
        if chunk.isEmpty { return chunk }
        var out = ""
        out.reserveCapacity(chunk.utf8.count + 8)
        // By scalar, not by `Character`: a CRLF is one grapheme cluster
        // in Swift, so a Character loop never sees the line feed on its
        // own — the same trap that let a filename inject a header.
        for scalar in chunk.unicodeScalars {
            if atLineStart, scalar == "." { out.unicodeScalars.append(".") }
            out.unicodeScalars.append(scalar)
            // Only a line feed starts a line. A lone CR does not: in a
            // message that is a stray byte, and treating it as a line
            // break would stuff a dot that is not at a line start.
            atLineStart = scalar == "\n"
        }
        return out
    }
}
