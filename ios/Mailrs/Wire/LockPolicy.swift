import Foundation

/// When the app asks for Face ID.
///
/// Split out and pure because the alternative is a rule that can only be
/// exercised by putting a phone down and picking it back up. Every
/// decision here is one comparison, and every one of them is a way to
/// get locked out of your own mail or to leave it open on a table.
enum LockPolicy {
    /// The window in which returning to the app does *not* ask again.
    ///
    /// Switching to Safari to read a link, or to Photos to attach one,
    /// is part of using a mail client — asking for a face on the way
    /// back from a five-second errand is how people turn the feature
    /// off. A minute is long enough to cover the errand and short
    /// enough that a phone left on a desk is not open.
    static let grace: TimeInterval = 60

    /// A cold launch always locks when the setting is on: there is no
    /// "just now" to be within.
    static func locksOnLaunch(enabled: Bool) -> Bool {
        enabled
    }

    /// `backgroundedAt` is when the app last left the foreground, `nil`
    /// if it has not since being unlocked.
    static func locksOnReturn(
        enabled: Bool, backgroundedAt: Date?, now: Date, grace: TimeInterval = grace
    ) -> Bool {
        guard enabled else { return false }
        guard let backgroundedAt else { return false }
        // A clock that moved backwards — a time-zone change, an NTP
        // step — must not read as "no time has passed". Anything not
        // clearly inside the window locks.
        let elapsed = now.timeIntervalSince(backgroundedAt)
        guard elapsed >= 0 else { return true }
        return elapsed > grace
    }
}
