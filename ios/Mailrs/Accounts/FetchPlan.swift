import Foundation

/// What to ask a folder for, given what is already held.
///
/// Its own type because the decision is where this goes wrong, and it
/// needs no socket to check. Three cases, and the middle one is the
/// one every client gets wrong once.
enum FetchPlan: Equatable {
    /// Never read this folder: everything in it.
    /// Never read this folder: the newest `count` of it.
    ///
    /// **Not everything.** A first sync of a mailbox with fifty
    /// thousand messages would fetch fifty thousand header blocks —
    /// hundreds of megabytes, many minutes, and a row list far past
    /// what this device stores in one go. Every mail client fetches a
    /// window and offers to go further; this fetches the window.
    ///
    /// By **sequence number**, not uid, because "the last five hundred
    /// messages" is what a sequence number means and there is no uid
    /// arithmetic that says it — uids have gaps wherever anything was
    /// ever deleted.
    case newest(count: Int, exists: Int)
    /// Read before, and the server's numbering still means what it
    /// meant: only what arrived since.
    case since(uid: UInt32)
    /// The server renumbered the folder.
    ///
    /// **`UIDVALIDITY` changed, so every uid held is meaningless** —
    /// uid 4390 is not the message it was, and asking for "everything
    /// after 4390" would skip mail or fetch the wrong thing. The
    /// folder is read from the start again.
    ///
    /// Not a fault and not rare: providers renumber after a restore, a
    /// migration, or a mailbox rename.
    case renumbered(count: Int, exists: Int)

    /// The range, and **which command it belongs to**.
    ///
    /// `UID FETCH 1:500` and `FETCH 1:500` mean completely different
    /// things — the first is uids, the second is positions in the
    /// folder — so the two travel together rather than as a string a
    /// caller pairs with a verb by hand.
    var range: String {
        switch self {
        case let .newest(count, exists): FetchPlan.window(count: count, exists: exists)
        case let .renumbered(count, exists): FetchPlan.window(count: count, exists: exists)
        case let .since(uid): "\(uid + 1):*"
        }
    }

    /// Whether `range` is uids. `false` means sequence numbers.
    var byUid: Bool {
        if case .since = self { return true }
        return false
    }

    /// How much of a folder a first pass reads.
    static let window = 500

    /// The last `count` positions, or the whole folder when it is
    /// smaller than that.
    static func window(count: Int, exists: Int) -> String {
        "\(max(1, exists - count + 1)):*"
    }
}

/// What this client remembers about a folder between passes.
struct FolderMark: Equatable, Codable {
    /// The validity the uids below were issued under.
    ///
    /// Kept **with** the uid and never apart: a uid without the
    /// validity that issued it is a number that means nothing, and
    /// storing them separately is how they drift.
    var uidValidity: UInt32
    /// The highest uid read.
    var highestUid: UInt32
    /// The lowest uid this device holds for the folder.
    ///
    /// Where "load earlier" starts asking from. Uids never move, but
    /// they leave gaps, so this is a boundary rather than a count of
    /// anything. Zero means nothing has been read yet.
    ///
    /// Defaulted, so a mark stored before this field existed still
    /// decodes — and defaulted to 0, which reads as "unknown" and makes
    /// the first "earlier" tap anchor itself from what is held.
    var lowestUid: UInt32 = 0
    /// How wide the next "earlier" range should be.
    ///
    /// Kept because it adapts: a range that came back nearly empty was
    /// mostly gaps, and the next one asks wider. Forgetting it makes
    /// every tap start narrow again in exactly the mailbox where narrow
    /// does not work.
    var earlierSpan: Int = EarlierPlan.firstSpan
}

extension FetchPlan {
    /// Decide, from what is held and what the server just said.
    /// - Parameter exists: how many messages the folder holds, from
    ///   `SELECT`. Needed because a first pass counts from the end.
    static func decide(
        mark: FolderMark?, serverValidity: UInt32, exists: Int = 0,
        window: Int = FetchPlan.window
    ) -> FetchPlan {
        guard let mark else { return .newest(count: window, exists: exists) }
        if mark.uidValidity != serverValidity {
            return .renumbered(count: window, exists: exists)
        }
        if mark.highestUid == 0 { return .newest(count: window, exists: exists) }
        return .since(uid: mark.highestUid)
    }
}
