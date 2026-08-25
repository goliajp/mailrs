import SwiftUI

struct RootView: View {
    @Environment(Session.self) private var session
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    /// The scheme after `preferredColorScheme` has had its say — so
    /// the tokens follow an explicit choice as readily as the system's.
    @Environment(\.colorScheme) private var colorScheme
    @Environment(SenderIcons.self) private var icons
    @Environment(Preferences.self) private var preferences
    /// The launch's own `.active` is not a return: `restore()` has
    /// just fetched, and refreshing on top of it doubles every cold
    /// start's traffic.
    @State private var hasBeenActive = false
    @State private var locked = false
    @State private var authenticating = false
    @State private var backgroundedAt: Date?

    /// A driven launch — the UI tests, and the simulator run lane —
    /// never locks. The prompt is a system sheet no test can answer, so
    /// leaving it in would wedge the suite rather than exercise it.
    private var lockEnabled: Bool {
        guard !ProcessInfo.processInfo.arguments.contains("-mailrsBaseURL") else { return false }
        return preferences.requiresBiometrics
    }

    var body: some View {
        Group {
            switch session.state {
            case .signedIn:
                // Two designs, chosen by how much width this scene
                // has — not by whether the hardware is an iPad. See
                // `PadLayout`. The phone's screen is unchanged.
                if PadLayout.splits(horizontalSizeClass) {
                    PadRootView()
                } else {
                    ConversationListView()
                }
            default:
                SignInView()
            }
        }
        .overlay {
            if locked {
                LockScreen(kind: BiometricLock.kind()) {
                    Task { await unlock() }
                }
                .transition(.opacity)
            }
        }
        .environment(\.theme, Theme.of(colorScheme))
        .task {
            // Wired here rather than held by the icon cache itself, so
            // the cache carries no credential and stops working when
            // the session does.
            icons.load = { [weak session] domain in
                await session?.icon(domain: domain)
            }
            if LockPolicy.locksOnLaunch(enabled: lockEnabled) { locked = true }
            await unlock()
            await session.restore()
        }
        .onChange(of: scenePhase) { _, phase in
            // Raised on the way out, not on the way back: iOS takes the
            // app switcher's picture as the app leaves, and a lock that
            // goes up afterwards is a lock with the mail in the
            // thumbnail behind it.
            guard phase == .active else {
                // Only an *unlocked* app records a leaving time. A lock
                // already up stays up however briefly the app was away:
                // otherwise cancelling the prompt, switching apps and
                // switching back would let someone in.
                if !locked { backgroundedAt = Date() }
                if lockEnabled { locked = true }
                return
            }
            // Only a return can lift the lock early. A `nil` here is the
            // launch's own `.active`, which must not undo the cold-start
            // lock the task above has just raised.
            if backgroundedAt != nil {
                let shouldLock = LockPolicy.locksOnReturn(
                    enabled: lockEnabled, backgroundedAt: backgroundedAt, now: Date())
                if !shouldLock { locked = false }
            }
            backgroundedAt = nil
            guard hasBeenActive else {
                hasBeenActive = true
                return
            }
            Task {
                await unlock()
                // Coming back to the app showed the mailbox as it was
                // when you left it — until push is live, this is the
                // only thing that makes a return show new mail. Behind
                // the lock, so a phone that never gets past it never
                // fetches either.
                guard !locked else { return }
                await session.refreshForeground()
            }
        }
    }

    /// Asks, once. Returns with the lock lifted or still up.
    ///
    /// Re-entrant by accident otherwise: the launch task and the scene
    /// phase both want to prompt, and two `LAContext` sheets at once is
    /// one the person can answer and one that stays on screen after.
    private func unlock() async {
        guard locked, !authenticating else { return }
        authenticating = true
        defer { authenticating = false }
        let passed = await BiometricLock.authenticate(
            reason: String(localized: "Unlock your mail"))
        guard passed else { return }
        locked = false
    }
}
