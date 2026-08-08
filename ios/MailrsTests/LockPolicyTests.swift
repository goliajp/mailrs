import Foundation
import Testing

@testable import Mailrs

@Suite("Lock policy")
struct LockPolicyTests {
    private let noon = Date(timeIntervalSince1970: 1_770_000_000)

    @Test("off means never")
    func offNeverLocks() {
        #expect(LockPolicy.locksOnLaunch(enabled: false) == false)
        #expect(
            LockPolicy.locksOnReturn(
                enabled: false, backgroundedAt: noon, now: noon.addingTimeInterval(3600))
                == false)
    }

    @Test("a cold launch always locks")
    func launchLocks() {
        #expect(LockPolicy.locksOnLaunch(enabled: true))
    }

    /// The distinction the grace window exists for: an app that never
    /// left the foreground has nothing to re-authenticate against.
    @Test("no time in the background is not a return")
    func neverLeftDoesNotLock() {
        #expect(
            LockPolicy.locksOnReturn(enabled: true, backgroundedAt: nil, now: noon) == false)
    }

    @Test("a short errand does not lock")
    func withinGraceStaysOpen() {
        for seconds in [0.0, 1, 30, 59.9, 60] {
            #expect(
                LockPolicy.locksOnReturn(
                    enabled: true, backgroundedAt: noon,
                    now: noon.addingTimeInterval(seconds)) == false,
                "\(seconds)s away should not lock")
        }
    }

    @Test("past the window it locks")
    func pastGraceLocks() {
        for seconds in [60.1, 61.0, 300, 86_400] {
            #expect(
                LockPolicy.locksOnReturn(
                    enabled: true, backgroundedAt: noon,
                    now: noon.addingTimeInterval(seconds)),
                "\(seconds)s away should lock")
        }
    }

    /// A clock that moved backwards — a time-zone change, an NTP step,
    /// or someone setting the date to escape the window — must not read
    /// as "no time has passed".
    @Test("a backwards clock locks")
    func backwardsClockLocks() {
        #expect(
            LockPolicy.locksOnReturn(
                enabled: true, backgroundedAt: noon, now: noon.addingTimeInterval(-1)))
        #expect(
            LockPolicy.locksOnReturn(
                enabled: true, backgroundedAt: noon, now: noon.addingTimeInterval(-86_400)))
    }

    @Test("the window is caller-adjustable")
    func graceIsAParameter() {
        #expect(
            LockPolicy.locksOnReturn(
                enabled: true, backgroundedAt: noon,
                now: noon.addingTimeInterval(5), grace: 1))
    }
}

@Suite("Locking preference")
@MainActor
struct LockPreferenceTests {
    private func defaults(_ name: String) -> UserDefaults {
        let suite = UserDefaults(suiteName: name)!
        suite.removePersistentDomain(forName: name)
        return suite
    }

    @Test("off until it is turned on")
    func defaultsOff() {
        #expect(Preferences(defaults: defaults("lock.default")).requiresBiometrics == false)
    }

    @Test("the choice survives a launch")
    func persists() {
        let name = "lock.persist"
        let store = defaults(name)
        Preferences(defaults: store).requiresBiometrics = true
        #expect(Preferences(defaults: store).requiresBiometrics)
    }

    /// The same trap the language preference fell into: `@Observable`
    /// runs `didSet` for the assignments in `init`, so loading a value
    /// wrote it straight back — and a default of `false` would then be
    /// indistinguishable from a choice of `false`.
    @Test("loading does not write")
    func loadingDoesNotWrite() {
        let name = "lock.noWriteOnLoad"
        let store = defaults(name)
        _ = Preferences(defaults: store)
        #expect(store.object(forKey: "mailrs.requiresBiometrics") == nil)
    }
}
