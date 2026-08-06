import Foundation

/// Paging the conversation list.
///
/// `GET /api/conversations` pages by keyset, not by cursor: `before_ts`
/// plus `limit`, and the server applies it as `latest_date < before_ts`
/// (`list_threads/paths.rs` passes `Some(ts - 1)` as an inclusive bound).
/// There is no `has_more` and no `next_cursor` to follow.
///
/// Which leaves one hazard, and it is not hypothetical: `last_date` is
/// whole seconds, so several threads share one. Asking for
/// `before_ts = oldest.lastDate` drops every sibling of that second that
/// did not fit on the page — silently, because a shorter list looks the
/// same as the end of the mailbox. `kevy-patterns.md` measured 929 such
/// collisions over 30k rows on this data.
///
/// So the next page asks for `oldest.lastDate + 1`, deliberately
/// re-requesting the boundary second, and `merge` drops what is already
/// held. The overlap costs a few rows; the alternative loses mail.
enum ThreadPage {
    /// The `before_ts` that will not skip the boundary second.
    static func nextBefore(after rows: [Wire.Conversation]) -> Int64? {
        guard let oldest = rows.last else { return nil }
        return oldest.lastDate + 1
    }

    struct Merged {
        let rows: [Wire.Conversation]
        /// Whether the page carried anything not already held.
        ///
        /// The termination condition, and it has to be this rather than a
        /// count: re-requesting the boundary second means a page can come
        /// back full of rows already on screen, and paging on
        /// "was it a full page?" would then ask for the same second
        /// forever. A page with nothing new is the end.
        let progressed: Bool
    }

    static func merge(_ existing: [Wire.Conversation], with incoming: [Wire.Conversation]) -> Merged {
        var seen = Set(existing.map(\.threadId))
        var rows = existing
        var progressed = false
        for row in incoming where !seen.contains(row.threadId) {
            seen.insert(row.threadId)
            rows.append(row)
            progressed = true
        }
        return Merged(rows: rows, progressed: progressed)
    }
}
