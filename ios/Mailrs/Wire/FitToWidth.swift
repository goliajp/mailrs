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
    /// A floor low enough to be a guard and nothing else.
    ///
    /// It was **0.45**, and that was the bug. 420 messages from one
    /// newsletter in this mailbox declare 1080px; fitting that into a
    /// 358pt card needs 0.33, the floor clamped it to 0.45, and the
    /// right-hand **26% of every one of them was off the edge** — with
    /// the body's scroll view disabled, so it could not be panned to
    /// either. A guard against "too small to read" produced "impossible
    /// to read", which is worse.
    ///
    /// The web's copy of this rule states the condition that makes a
    /// floor safe: *"whatever the floor leaves over stays reachable by
    /// scrolling the body sideways, which is why the container this
    /// renders into must not be overflow-hidden."* This port took the
    /// number and not the condition — the body's scroll view was
    /// disabled — so here the floor did not leave the remainder
    /// reachable, it deleted it.
    ///
    /// The remainder is reachable now (`MessageBodyView` pans and
    /// zooms), and the floor is still lowered, because on a phone
    /// fitting the column beats panning sideways for every line of a
    /// newsletter. That makes the two numbers differ on purpose:
    /// **web 0.45, here 0.1**. If the web client ever renders into a
    /// phone-width column, it wants this number, not its own.
    static let minScale = 0.1

    /// Never greater than 1: a narrow message is left at its own size
    /// rather than blown up to fill the screen.
    static func scale(contentWidth: Double, hostWidth: Double) -> Double {
        guard contentWidth.isFinite, hostWidth.isFinite else { return 1 }
        guard contentWidth > 0, hostWidth > 0 else { return 1 }
        guard contentWidth > hostWidth else { return 1 }
        return max(minScale, hostWidth / contentWidth)
    }
}
