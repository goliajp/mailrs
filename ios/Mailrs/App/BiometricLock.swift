import LocalAuthentication
import SwiftUI

/// Face ID, and what to do when it is not there.
///
/// `deviceOwnerAuthentication`, not `…WithBiometrics`: the passcode
/// fallback is the difference between a lock and a lockout. A face that
/// stops being recognised — a mask, a bad angle, a cut on a finger —
/// must not put someone's mail out of reach.
///
/// This deliberately does **not** put the session token behind a
/// biometric Keychain item. `biometryCurrentSet` invalidates the item
/// when the enrolled face or fingers change, and the symptom is being
/// silently signed out with no way to tell why. The token stays where
/// it was; this gates the screen in front of it.
@MainActor
enum BiometricLock {
    enum Kind {
        case faceID
        case touchID
        case opticID
        case passcodeOnly
        case none

        /// Named, because "Biometrics" is a word no phone uses about
        /// itself and everyone's phone tells them which one it has.
        var label: LocalizedStringKey {
            switch self {
            case .faceID: return "Require Face ID"
            case .touchID: return "Require Touch ID"
            case .opticID: return "Require Optic ID"
            case .passcodeOnly: return "Require passcode"
            case .none: return "Require unlocking"
            }
        }

        var symbol: [Lucide.Element] {
            switch self {
            case .faceID, .opticID: return Lucide.scanFace
            case .touchID: return Lucide.fingerprint
            case .passcodeOnly, .none: return Lucide.lockKeyhole
            }
        }
    }

    /// What this device can actually do, asked once per call rather than
    /// cached: enrolment changes while the app is running.
    static func kind() -> Kind {
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            return .none
        }
        switch context.biometryType {
        case .faceID: return .faceID
        case .touchID: return .touchID
        case .opticID: return .opticID
        default: return .passcodeOnly
        }
    }

    /// True when the setting is worth offering at all: a device with no
    /// passcode has nothing to authenticate against, and a toggle that
    /// cannot be honoured is worse than no toggle.
    static var isAvailable: Bool {
        kind() != .none
    }

    /// The prompt. Returns whether the person got through it.
    ///
    /// A cancel and a failure are the same answer here — both mean
    /// "still locked" — so the caller has one thing to check rather
    /// than an error to interpret.
    static func authenticate(reason: String) async -> Bool {
        let context = LAContext()
        // Shown under the Face ID sheet when it falls back to the
        // passcode. Left as the system default wording otherwise.
        context.localizedCancelTitle = String(localized: "Cancel")
        do {
            return try await context.evaluatePolicy(.deviceOwnerAuthentication,
                                                    localizedReason: reason)
        } catch {
            return false
        }
    }
}
