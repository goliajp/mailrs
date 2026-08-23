import Foundation

/// A colour per mailbox, so a merged list can say which is which.
///
/// Derived from the id rather than stored: the same account is the
/// same colour on every launch, and there is nothing to keep in step.
enum AccountColour {
    static let palette = [
        "#4285f4", "#12b7f5", "#ea4335", "#34a853", "#a142f4",
        "#f4b400", "#ff6d00", "#00897b",
    ]

    static func forId(_ id: String) -> String {
        var h: UInt64 = 0xcbf2_9ce4_8422_2325
        for b in id.utf8 {
            h ^= UInt64(b)
            h = h &* 0x100_0000_01b3
        }
        return palette[Int(h % UInt64(palette.count))]
    }
}
