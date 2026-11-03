import Foundation

/// Paging the conversation list.
///
/// Every row of `GET /api/conversations` (and of `GET /api/quarantine`)
/// carries a `cursor`, and the next page is asked for with the last row's
/// one as `cursor=`. The server orders rows by time and, within one
/// second, by a key of its own, and starts the next page strictly after
/// the cursor's row: a second that spans two pages loses nothing and
/// repeats nothing.
///
/// What it replaced: `before_ts`, which compares whole seconds. Several
/// threads share a second (production measured 929 such collisions over
/// 30k rows), so asking for the oldest row's own second dropped the
/// siblings that did not fit, and asking for one second past it — what
/// this did until the cursor — re-requested the boundary second, which
/// only works while that second holds fewer rows than a page. With more,
/// the next page came back as the same rows, read as the end, and the
/// rest of that second was never shown.
///
/// A row without a cursor (cached before the server stamped one) still
/// pages the old way, which the server keeps for clients not yet moved.
/// `merge` drops what is already held either way.
enum ThreadPage {
    /// Where the page after `rows` starts.
    enum Next: Equatable {
        case cursor(String)
        /// The keyset of old: one second past the oldest row.
        case before(Int64)
    }

    static func next(after rows: [Wire.Conversation]) -> Next? {
        guard let oldest = rows.last else { return nil }
        if let cursor = oldest.cursor { return .cursor(cursor) }
        return .before(oldest.lastDate + 1)
    }

    /// The query items that ask for the page `next` names.
    static func queryItems(_ next: Next?) -> [URLQueryItem] {
        switch next {
        case .cursor(let c): return [URLQueryItem(name: "cursor", value: c)]
        case .before(let ts): return [URLQueryItem(name: "before_ts", value: String(ts))]
        case nil: return []
        }
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

extension Array {
    /// The element at `index`, or nil when it is out of bounds.
    ///
    /// The thread's chevrons ask for "the row before this one" at the
    /// top of the list and "the row after" at the bottom, where the
    /// honest answer is nothing rather than a crash.
    subscript(safe index: Int) -> Element? {
        guard indices.contains(index) else { return nil }
        return self[index]
    }
}
