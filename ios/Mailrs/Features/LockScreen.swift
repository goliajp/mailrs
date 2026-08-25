import SwiftUI

/// What stands in front of the mail while the app is locked.
///
/// Opaque, and drawn over everything: the point of a lock is that the
/// subject lines are not readable over its shoulder. That also covers
/// the app switcher's snapshot, which iOS takes as the app leaves the
/// foreground — the moment the lock goes up is before the picture.
///
/// It offers the prompt again rather than only appearing after a
/// failure: a cancelled Face ID is the ordinary case (the phone was
/// picked up by accident), and the way back must be one tap.
struct LockScreen: View {
    let kind: BiometricLock.Kind
    let onUnlock: () -> Void

    var body: some View {
        ZStack {
            Color.pageBackground
                .ignoresSafeArea()
            VStack(spacing: 20) {
                LucideIcon(elements: kind.symbol, size: 44)
                    .foregroundStyle(Color.accentColor)
                Text("Mailrs is locked")
                    .font(.headline)
                Button(action: onUnlock) {
                    Text("Unlock")
                        .frame(maxWidth: 220)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("unlock")
            }
        }
        // The mail behind it is not reachable by VoiceOver either — a
        // lock that only hides pixels is not a lock.
        .accessibilityAddTraits(.isModal)
    }
}
