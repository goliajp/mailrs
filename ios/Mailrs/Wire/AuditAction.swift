import Foundation

/// How an audit action reads.
///
/// The server writes dotted verbs — `alias.create`, `account.delete`
/// — and the family before the dot is what it filters on. Splitting
/// them here means the screen can offer the family as a filter and
/// still show the verb, rather than showing a string and hoping the
/// reader parses it.
enum AuditAction {
    /// The part before the first dot, which is the server's filter
    /// prefix. `alias.create` → `alias`.
    static func family(of action: String) -> String {
        guard let dot = action.firstIndex(of: ".") else { return action }
        return String(action[action.startIndex..<dot])
    }

    /// The part after it, as a word. `alias.create` → `create`.
    /// An action with no dot is its own verb.
    static func verb(of action: String) -> String {
        guard let dot = action.firstIndex(of: ".") else { return action }
        return String(action[action.index(after: dot)...])
    }

    /// Whether this action removed something — the rows worth finding
    /// in a hurry, and the ones a colour should mark.
    static func isDestructive(_ action: String) -> Bool {
        let verb = verb(of: action).lowercased()
        return verb.contains("delete") || verb.contains("remove") || verb.contains("revoke")
    }
}
