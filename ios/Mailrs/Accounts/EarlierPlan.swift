import Foundation

/// What to ask for when somebody wants the mail **before** what they
/// have.
///
/// The first pass takes a window from the end of the folder, so
/// everything older than that window has never been fetched and has no
/// way in. This is the way in.
///
/// **By uid span, not by position.** A sequence number is what a
/// message is *today*: anything deleted below it shifts it, so a window
/// remembered as "positions 400 to 500" points somewhere else by the
/// next pass. Uids never move. What they do instead is leave **gaps**
/// wherever something was deleted — so a span of 500 uids may hold five
/// messages or five hundred, and neither is a fault.
///
/// The span therefore adapts: a range that came back nearly empty was
/// mostly holes, and the next one asks wider. A range that came back
/// full asked about the right amount.
enum EarlierPlan {
    /// How many uids to reach back over on the first attempt.
    static let firstSpan = 200

    /// Beyond this a single fetch is large enough to be its own problem.
    static let maxSpan = 5_000

    /// How few is few enough to widen.
    static let thin = 10

    struct Ask: Equatable {
        /// The `UID FETCH` range, or nil when there is nothing older.
        var range: String?
        /// The span this asked about, to be carried to the next answer.
        var span: Int
    }

    /// Whether this device is already holding as much as it may.
    ///
    /// The cap drops the **oldest** rows, and "load earlier" fetches
    /// exactly those — so at the ceiling the two undo each other and
    /// the button spends a network round trip to change nothing. That
    /// is the worst of the three possible behaviours: worse than
    /// refusing, because it looks like it worked and did not, and it
    /// takes a person several taps to be sure.
    ///
    /// So this is asked **before** fetching, and a full device says
    /// so. It is a real limit, and the honest answer to a real limit
    /// is the limit.
    static func atCeiling(held: Int, ceiling: Int = MailboxApply.perAccount) -> Bool {
        held >= ceiling
    }

    /// - Parameter lowestHeldUid: the smallest uid this device already
    ///   has for the folder. **1 means the folder is exhausted**:
    ///   there is no uid below it.
    static func decide(lowestHeldUid: UInt32, span: Int = firstSpan) -> Ask {
        guard lowestHeldUid > 1 else { return Ask(range: nil, span: span) }
        let top = lowestHeldUid - 1
        let bottom = max(1, Int(top) - span + 1)
        return Ask(range: "\(bottom):\(top)", span: span)
    }

    /// The span for the next tap, given what the last one returned.
    ///
    /// Widened when the answer was thin, because thin means the range
    /// was mostly gaps and the same width would be thin again — a
    /// person tapping "earlier" five times to see one message would
    /// rightly call that broken.
    static func nextSpan(_ span: Int, returned: Int) -> Int {
        if returned >= thin { return span }
        return min(maxSpan, span * 4)
    }

    /// Whether the answer means the folder is finished.
    ///
    /// **Not "nothing came back"** — a range that is all gaps returns
    /// nothing and there may be plenty below it. It is finished when
    /// the range that was asked about reached uid 1.
    static func exhausted(_ ask: Ask) -> Bool { ask.range?.hasPrefix("1:") == true }
}
