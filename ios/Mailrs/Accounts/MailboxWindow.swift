import Foundation

/// What the end of the list offers.
///
/// There are two things a person can want there and they are not the
/// same: **show more of what is already here**, which is a `LIMIT` and
/// costs nothing, and **go and get older mail**, which is a network
/// round trip against a server that may be slow or asleep.
///
/// They used to be one button, and it read as one action. It was not,
/// and the slow one ran when the fast one would have done — on a
/// mailbox with anything in it, every time.
enum MailboxWindow {
    /// Whether the store had at least as many rows as were asked for,
    /// which is the only evidence available that it has more.
    ///
    /// A window that comes back full may be exactly full — one wasted
    /// query, and then the list settles. A window that comes back short
    /// is proof there is no more, and that is the direction that
    /// matters: it is what stops the list growing forever.
    static func moreHeld(returned: Int, asked: Int) -> Bool { returned >= asked }

    /// Whether to offer fetching older mail from the server.
    ///
    /// Three conditions, and each is a different mistake avoided:
    ///
    /// - not while more is held, or the slow action is offered when the
    ///   fast one would do;
    /// - not on an empty list, because "earlier" than nothing has no
    ///   anchor to reach back from — the ordinary pass is what gives a
    ///   folder one;
    /// - not while searching, because a fetch against a filtered list
    ///   brings back mail that will not be shown, and looks like it did
    ///   nothing.
    static func offersEarlier(moreHeld: Bool, shownCount: Int, searching: Bool) -> Bool {
        !moreHeld && shownCount > 0 && !searching
    }
}
