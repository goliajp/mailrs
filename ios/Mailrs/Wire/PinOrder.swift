import Foundation

/// Pinned threads, first.
///
/// `pinned` is a declared column on the membership table with an axis
/// of its own, and the web client has always drawn those rows at the
/// top of the list. This client decoded the field and did nothing with
/// it: a thread pinned at the desk sat wherever its date put it on the
/// phone, which is the one place the pin was supposed to help.
///
/// The server returns the list by activity, so the arrangement is the
/// client's to make — the same division the web draws in `list-rows.ts`.
enum PinOrder {
    /// Pinned rows first, each group in the order the server gave.
    ///
    /// Stable within a group: the server sorted by activity and that
    /// order is the answer, so this only lifts a set out of it. A
    /// comparison sort keyed on `pinned` alone would be free to shuffle
    /// same-key rows and the list would reorder itself between
    /// refreshes for no visible reason.
    static func arrange<Row>(_ rows: [Row], pinned: (Row) -> Bool) -> [Row] {
        var top: [Row] = []
        var rest: [Row] = []
        top.reserveCapacity(rows.count)
        rest.reserveCapacity(rows.count)
        for row in rows {
            if pinned(row) {
                top.append(row)
                continue
            }
            rest.append(row)
        }
        // Nothing pinned is the common case, and rebuilding the array
        // for it would rewrite the identity of every row on every
        // refresh.
        if top.isEmpty { return rows }
        return top + rest
    }
}
