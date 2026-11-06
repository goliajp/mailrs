import Foundation
import Observation

/// Whether a person removed this phone's pushes from another device, and
/// whether the reader here asked for them back.
///
/// The app hands its token to the server on every signed-in launch. The
/// server refuses a removed token (`410 removed`) unless the registration
/// says the reader turned pushes on here (`resume`), so a launch can never
/// undo a removal, whatever this remembers. This only stops the launches
/// asking at all once refused, and lets the settings page say why pushes
/// stopped and offer to turn them on again.
@Observable
@MainActor
final class PushRemoval {
    static let shared = PushRemoval(defaults: .standard)

    private let defaults: UserDefaults
    private(set) var removedElsewhere: Bool
    private var resuming: Bool

    init(defaults: UserDefaults) {
        self.defaults = defaults
        removedElsewhere = defaults.bool(forKey: Self.removedKey)
        resuming = defaults.bool(forKey: Self.resumingKey)
    }

    /// The `resume` to register with, or nil to not register.
    func registration() -> Bool? {
        if resuming { return true }
        if removedElsewhere { return nil }
        return false
    }

    func answered(_ outcome: PushTokenOutcome) {
        switch outcome {
        case .registered: store(removed: false, resuming: false)
        case .removed: store(removed: true, resuming: false)
        }
    }

    /// The reader asked for pushes here again.
    func turnOnAgain() {
        store(removed: removedElsewhere, resuming: true)
    }

    /// Signed out: what held for that account says nothing about the next.
    func forget() {
        store(removed: false, resuming: false)
    }

    private func store(removed: Bool, resuming: Bool) {
        removedElsewhere = removed
        self.resuming = resuming
        defaults.set(removed, forKey: Self.removedKey)
        defaults.set(resuming, forKey: Self.resumingKey)
    }

    private static let removedKey = "push.removedElsewhere"
    private static let resumingKey = "push.resuming"
}
