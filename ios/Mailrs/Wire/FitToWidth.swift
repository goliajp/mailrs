import Foundation

/// How much to shrink an email so it fits the width it is given.
///
/// The same rule the web client applies, deliberately: HTML mail is
/// authored against a fixed pixel width — a survey of 400 messages in
/// this mailbox found them clustering at 600, 640, 650, 680, 700 and
/// 768 px — and none of those fit a phone. Reflowing is not on the
/// table, because the layout is tables and absolute widths and a client
/// that reflows shows something the sender never composed.
///
/// Ported rather than reimplemented so the two clients cannot drift into
/// disagreeing about the same question. If this changes, change
/// `web/src/lib/fit-to-width.ts` in the same commit.
enum FitToWidth {
    /// A guard against a pathological width — a 3000px canvas, a runaway
    /// `<pre>` — being scaled into an unreadable smear. No width in that
    /// survey reaches it, so in practice every real message fits exactly.
    static let minScale = 0.45

    /// Never greater than 1: a narrow message is left at its own size
    /// rather than blown up to fill the screen.
    static func scale(contentWidth: Double, hostWidth: Double) -> Double {
        guard contentWidth.isFinite, hostWidth.isFinite else { return 1 }
        guard contentWidth > 0, hostWidth > 0 else { return 1 }
        guard contentWidth > hostWidth else { return 1 }
        return max(minScale, hostWidth / contentWidth)
    }
}
