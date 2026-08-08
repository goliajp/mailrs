import Foundation

/// When a typed string is worth asking the server about.
enum SearchRule {
    /// Below this, a query matches so much that the answer is noise, and
    /// the server's own LIKE stage refuses it anyway. Flooring it here
    /// saves the round trip rather than duplicating a decision: one
    /// character is not a search on either side.
    static let minimumLength = 2

    /// The query to send, or nil for "not a search — show the list".
    ///
    /// Trimmed, because a trailing space arrives on every phone keyboard
    /// after a word and would otherwise make " a" a two-character query.
    static func query(from text: String) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.count >= minimumLength else { return nil }
        return trimmed
    }
}
