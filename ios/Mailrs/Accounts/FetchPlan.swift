import Foundation

/// What to ask a folder for, given what is already held.
///
/// Its own type because the decision is where this goes wrong, and it
/// needs no socket to check. Three cases, and the middle one is the
/// one every client gets wrong once.
enum FetchPlan: Equatable {
    /// Never read this folder: everything in it.
    case everything
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
    case renumbered

    /// The range for a `UID FETCH`.
    var range: String {
        switch self {
        case .everything, .renumbered: "1:*"
        case let .since(uid): "\(uid + 1):*"
        }
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
}

extension FetchPlan {
    /// Decide, from what is held and what the server just said.
    static func decide(mark: FolderMark?, serverValidity: UInt32) -> FetchPlan {
        guard let mark else { return .everything }
        if mark.uidValidity != serverValidity { return .renumbered }
        if mark.highestUid == 0 { return .everything }
        return .since(uid: mark.highestUid)
    }
}
