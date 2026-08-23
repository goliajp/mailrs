import Foundation

/// What to fetch from a POP3 mailbox, and what to remember afterwards.
///
/// Pure, because the two mistakes available here are both silent. A
/// client that remembers message **numbers** re-downloads the mailbox
/// every time somebody deletes anything, since POP3 renumbers on every
/// session. One that remembers every uidl it has ever seen grows a set
/// that never shrinks, and after a year the bookkeeping is larger than
/// the mailbox.
enum POP3Plan {
    struct Plan: Equatable {
        /// Message numbers to fetch, oldest first so the list fills in
        /// order.
        var fetch: [Int]
        /// What to keep of the old set: the ids still on the server.
        ///
        /// Ids that have gone are dropped rather than kept — a message
        /// deleted on the server cannot come back, and keeping its id
        /// forever is how the set outgrows the mailbox.
        var keep: Set<String>
        /// How many were left for a later pass, so the caller can say
        /// so.
        var deferred: Int
    }

    /// - Parameter limit: how many to fetch in one pass. A first sync
    ///   of a mailbox with thousands of messages must not download all
    ///   of them before anything appears on screen; the newest are the
    ///   ones somebody is looking for.
    static func decide(server: [POP3.Uidl], seen: Set<String>, limit: Int = 200) -> Plan {
        let present = Set(server.map(\.id))
        let unseen = server.filter { !seen.contains($0.id) }
        // Newest first to choose, oldest first to fetch: message
        // numbers run in arrival order, so the high ones are the recent
        // ones — and a list that fills from the top reads as mail
        // arriving.
        let chosen = unseen.sorted { $0.number > $1.number }.prefix(limit)
        return Plan(
            fetch: chosen.map(\.number).sorted(),
            keep: seen.intersection(present),
            deferred: unseen.count - chosen.count)
    }
}
