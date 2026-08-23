import Foundation

/// Turning a list of uids into something a server will accept.
///
/// Two problems, and the second is the one that bites late. A command
/// naming five thousand uids one by one is a line tens of kilobytes
/// long, and servers commonly refuse over-long lines — so the mailbox
/// that most needs its flags refreshed is the one where the refresh
/// silently stops working.
///
/// Runs are collapsed first because uids in a folder are mostly
/// consecutive, and `12:4000` says the same as four thousand numbers.
/// What is left is chunked, because even collapsed a sparse mailbox
/// can be long.
enum UidRanges {
    /// Well under what any server is likely to refuse.
    static let maxChars = 900

    /// `1,2,3,7,8,20` becomes `1:3,7:8,20`.
    ///
    /// Sorted first: a set has no order, and a server reading
    /// `20,1:3` gets a valid but pointlessly awkward sequence.
    static func collapse(_ uids: [UInt32]) -> String {
        let sorted = Array(Set(uids)).sorted()
        guard let first = sorted.first else { return "" }
        var out: [String] = []
        var start = first
        var previous = first
        func flush() {
            if start == previous {
                out.append(String(start))
            } else {
                out.append("\(start):\(previous)")
            }
        }
        for uid in sorted.dropFirst() {
            if uid == previous + 1 {
                previous = uid
                continue
            }
            flush()
            start = uid
            previous = uid
        }
        flush()
        return out.joined(separator: ",")
    }

    /// The same, split so no one command is too long.
    ///
    /// Split on **whole runs**, never inside one: half of `1:3` is not
    /// a range, and a server would read whatever the halves happen to
    /// spell.
    static func batches(_ uids: [UInt32], maxChars: Int = maxChars) -> [String] {
        let whole = collapse(uids)
        if whole.isEmpty { return [] }
        if whole.count <= maxChars { return [whole] }
        var out: [String] = []
        var current = ""
        for run in whole.split(separator: ",").map(String.init) {
            if !current.isEmpty, current.count + 1 + run.count > maxChars {
                out.append(current)
                current = ""
            }
            if !current.isEmpty { current += "," }
            current += run
        }
        if !current.isEmpty { out.append(current) }
        return out
    }
}
