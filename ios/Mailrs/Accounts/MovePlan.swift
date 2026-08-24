import Foundation

/// Which commands a move needs on this particular server.
///
/// `MOVE` (RFC 6851) where the server has it, and the older three-step
/// dance where it does not — and the difference matters more than it
/// looks:
///
/// **A bare `EXPUNGE` removes every message in the folder flagged
/// `\Deleted`**, including ones somebody else's client flagged and has
/// not expunged yet. `UID EXPUNGE` (RFC 4315) removes only the one
/// named. Where neither `MOVE` nor `UIDPLUS` is offered, the message is
/// flagged and **left** rather than expunged: it disappears from the
/// list either way, and no other message is taken with it.
///
/// Decided here rather than inside the session because it is the part
/// that varies between servers and the part that can lose somebody
/// else's mail, and neither of those should need a socket to check.
enum MovePlan {
    enum Step: Equatable {
        /// Send this and wait for its tagged completion.
        case command(String)
        /// `UID STORE … +FLAGS (\Deleted)`, which the session owns.
        case markDeleted
    }

    static func steps(uid: UInt32, folder: String, capabilities: Set<String>) -> [Step] {
        if capabilities.contains("MOVE") {
            return [.command("UID MOVE \(uid) \(IMAP.quoted(folder))")]
        }
        var out: [Step] = [
            .command("UID COPY \(uid) \(IMAP.quoted(folder))"),
            .markDeleted,
        ]
        // Only where the server can be told *which* one.
        if capabilities.contains("UIDPLUS") {
            out.append(.command("UID EXPUNGE \(uid)"))
        }
        return out
    }
}
