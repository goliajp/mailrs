import Foundation

/// How much of a window's mail aligned.
///
/// The one number a DMARC screen exists for: mail that did not align
/// is mail a receiver was entitled to reject, so this is deliverability
/// rather than a security score.
enum AlignmentRate {
    /// `nil` when nothing was counted — zero of zero is not 0%, it is
    /// "no reports", and a screen that showed 0% would announce a
    /// failure that has not happened.
    static func fraction(passing: UInt64, total: UInt64) -> Double? {
        guard total > 0 else { return nil }
        return Double(passing) / Double(total)
    }

    /// Percent, rounded the way a reader reads it: 99.94% stays 99.9%
    /// rather than rounding up to 100 and claiming perfection.
    static func percentText(passing: UInt64, total: UInt64) -> String? {
        guard let fraction = fraction(passing: passing, total: total) else { return nil }
        let percent = (fraction * 1000).rounded(.down) / 10
        return String(format: "%.1f%%", percent)
    }
}
