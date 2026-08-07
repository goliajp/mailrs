import Foundation

/// Which messages in a thread show their full card.
///
/// Apple Mail and Gmail both open a long thread with everything but
/// the newest message folded to a header line — the thread is context,
/// the last message is the reason you came. The wire carries no
/// per-message read state, so recency is the whole rule.
///
/// Expansion is derived, not latched: the state is the set of messages
/// the reader explicitly toggled, and a message shows expanded when
/// (is the last) XOR (was toggled). No effect ever has to reconcile a
/// stored set against a new thread — the override set resets and the
/// derivation is right again.
enum ThreadCollapse {
    static func isExpanded(uid: UInt32, lastUid: UInt32?, toggled: Set<UInt32>) -> Bool {
        (uid == lastUid) != toggled.contains(uid)
    }

    /// The folded card's one line of body: whitespace collapsed, hard
    /// bounded — a header row, not a second body.
    static func snippet(_ text: String?, limit: Int = 80) -> String {
        guard let text else { return "" }
        let folded = text
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespaces)
        if folded.count <= limit { return folded }
        return String(folded.prefix(limit)) + "…"
    }
}
