import Foundation

/// What a pass learned about messages this device already had.
///
/// Pure, because the deletion rule is the one that can lose somebody's
/// mail if it is wrong, and it should be checkable without a server.
enum MailboxRefresh {
    /// Apply a flag answer to the rows of one folder.
    ///
    /// Two things happen, and the second is the dangerous one:
    ///
    /// - a row whose uid came back with a different `\Seen` is
    ///   updated, so a message read on a laptop stops being bold here;
    /// - **a row whose uid was asked about and did not come back is
    ///   removed**, because the server no longer has it.
    ///
    /// The asking matters: only rows in `asked` may be removed. A row
    /// from another folder, or one that was never in the question,
    /// cannot be deleted by an answer that was not about it — which is
    /// what stops a partial or interrupted fetch from emptying a list.
    static func apply(
        held: [MailboxRow], accountId: String, folder: String, asked: Set<UInt32>,
        answer: [UInt32: Bool]
    ) -> [MailboxRow] {
        held.compactMap { row in
            guard row.accountId == accountId, row.folder == folder else { return row }
            guard asked.contains(row.uid) else { return row }
            guard let seen = answer[row.uid] else { return nil }
            guard seen != row.seen else { return row }
            var changed = row
            changed.seen = seen
            return changed
        }
    }
}
