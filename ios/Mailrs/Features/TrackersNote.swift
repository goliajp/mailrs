import SwiftUI

/// Above a message whose beacons were taken out: how many, so the reader
/// knows the sender wanted to learn it was opened, and did not.
struct TrackersNote: View {
    let count: Int

    var body: some View {
        if count > 0 {
            Label("Trackers blocked: \(count)", systemImage: "eye.slash")
                .font(.caption)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("trackers.blocked")
        }
    }
}
