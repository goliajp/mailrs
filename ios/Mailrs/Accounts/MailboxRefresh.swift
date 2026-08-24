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
    ///
    /// **No production caller since the rows moved into SQLite** — that
    /// path uses `decide` and addresses the rows. This is kept because
    /// it is the readable statement of the rule *and* because it is
    /// implemented in terms of `decide`, so its tests are that
    /// function's tests. Delete both together or neither.
    static func apply(
        held: [MailboxRow], accountId: String, folder: String, asked: Set<UInt32>,
        answer: [UInt32: Bool]
    ) -> [MailboxRow] {
        let decision = decide(asked: asked, answer: answer)
        return held.compactMap { row in
            guard row.accountId == accountId, row.folder == folder else { return row }
            guard !decision.gone.contains(row.uid) else { return nil }
            guard let seen = decision.flags[row.uid], seen != row.seen else { return row }
            var changed = row
            changed.seen = seen
            return changed
        }
    }

    /// What a pass decided, without the rows.
    ///
    /// - `gone`: uids the server was asked about and did not
    ///   acknowledge — and **only** those; see `apply`.
    /// - `flags`: the `\Seen` state the server reported.
    struct Decision: Equatable {
        let gone: Set<UInt32>
        let flags: [UInt32: Bool]
    }

    /// The same rule, as a decision rather than as a new list.
    ///
    /// `apply` rewrites every row it is handed, which is what a store
    /// that keeps its rows in one blob needs. A store that can address
    /// a row wants to be told *which* rows changed instead, so the rule
    /// lives here once and both callers read it from the same place — a
    /// second copy of "which uid may be deleted" is the copy that
    /// eventually deletes somebody's mail.
    static func decide(asked: Set<UInt32>, answer: [UInt32: Bool]) -> Decision {
        Decision(
            gone: asked.filter { answer[$0] == nil },
            flags: answer.filter { asked.contains($0.key) })
    }
}
