import SwiftUI

/// The icons, taken from Lucide — the set the web client already uses
/// (`lucide-react`, `web/package.json`). ISC licensed.
///
/// Copied verbatim from `node_modules/lucide-react/dist/esm/icons/*.mjs`
/// so an update is a paste rather than a redraw, which is also why
/// `SVGPath` handles arcs instead of a script flattening them first.
///
/// They are stroked, not filled: Lucide is a 24×24 grid with a 2px
/// round-capped stroke, and that is the whole of its look. SF Symbols
/// mixed with them read as two different sets on one screen, which is
/// what the settings list looked like.
enum Lucide {
    /// One element of an icon. Lucide draws with paths, circles and
    /// rounded rectangles; nothing else appears in the ones used here.
    enum Element {
        case path(String)
        case circle(x: CGFloat, y: CGFloat, r: CGFloat)
        case rect(x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat, r: CGFloat)
    }

    /// `users` — the mailboxes that exist.
    static let users: [Element] = [
        .path("M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"),
        .path("M16 3.128a4 4 0 0 1 0 7.744"),
        .path("M22 21v-2a4 4 0 0 0-3-3.87"),
        .circle(x: 9, y: 7, r: 4),
    ]

    /// `split` — one address that becomes another.
    static let split: [Element] = [
        .path("M16 3h5v5"),
        .path("M8 3H3v5"),
        .path("M12 22v-8.3a4 4 0 0 0-1.172-2.872L3 3"),
        .path("m15 9 6-6"),
    ]

    /// `mails` — a group address is several deliveries of one message.
    static let mails: [Element] = [
        .path("M17 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-8a2 2 0 0 1 1-1.732"),
        .path("m22 5.5-6.419 4.179a2 2 0 0 1-2.162 0L7 5.5"),
        .rect(x: 7, y: 3, w: 15, h: 12, r: 2),
    ]

    static let globe: [Element] = [
        .circle(x: 12, y: 12, r: 10),
        .path("M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"),
        .path("M2 12h20"),
    ]

    /// `lock-keyhole` — permissions. Distinct from DMARC's shield on
    /// purpose: one is what someone may do, the other is whether a
    /// message was who it said.
    static let lockKeyhole: [Element] = [
        .circle(x: 12, y: 16, r: 1),
        .rect(x: 3, y: 10, w: 18, h: 12, r: 2),
        .path("M7 10V7a5 5 0 0 1 10 0v3"),
    ]

    /// `send` — the outbound queue.
    static let send: [Element] = [
        .path("M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z"),
        .path("m21.854 2.147-10.94 10.939"),
    ]

    /// `pen-line` — the signature.
    static let penLine: [Element] = [
        .path("M12 20h9"),
        .path("M16.376 3.622a1 1 0 0 1 3.002 3.002L7.368 18.635a2 2 0 0 1-.855.506l-2.872.838a.5.5 0 0 1-.62-.62l.838-2.872a2 2 0 0 1 .506-.854z"),
    ]

    static let shieldCheck: [Element] = [
        .path("M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"),
        .path("m9 12 2 2 4-4"),
    ]

    /// `scroll-text` — the audit log.
    static let scrollText: [Element] = [
        .path("M15 12h-5"),
        .path("M15 8h-5"),
        .path("M19 17V5a2 2 0 0 0-2-2H4"),
        .path("M8 21h12a2 2 0 0 0 2-2v-1a1 1 0 0 0-1-1H11a1 1 0 0 0-1 1v1a2 2 0 1 1-4 0V5a2 2 0 1 0-4 0v2a1 1 0 0 0 1 1h3"),
    ]

    /// `scan-face` — Face ID.
    static let scanFace: [Element] = [
        .path("M3 7V5a2 2 0 0 1 2-2h2"),
        .path("M17 3h2a2 2 0 0 1 2 2v2"),
        .path("M21 17v2a2 2 0 0 1-2 2h-2"),
        .path("M7 21H5a2 2 0 0 1-2-2v-2"),
        .path("M8 14s1.5 2 4 2 4-2 4-2"),
        .path("M9 9h.01"),
        .path("M15 9h.01"),
    ]

    /// `fingerprint-pattern` — Touch ID.
    static let fingerprint: [Element] = [
        .path("M12 10a2 2 0 0 0-2 2c0 1.02-.1 2.51-.26 4"),
        .path("M14 13.12c0 2.38 0 6.38-1 8.88"),
        .path("M17.29 21.02c.12-.6.43-2.3.5-3.02"),
        .path("M2 12a10 10 0 0 1 18-6"),
        .path("M2 16h.01"),
        .path("M21.8 16c.2-2 .131-5.354 0-6"),
        .path("M5 19.5C5.5 18 6 15 6 12a6 6 0 0 1 .34-2"),
        .path("M8.65 22c.21-.66.45-1.32.57-2"),
        .path("M9 6.8a6 6 0 0 1 9 5.2v2"),
    ]

    static let keyRound: [Element] = [
        .path("M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z"),
        .circle(x: 16.5, y: 7.5, r: 0.5),
    ]
}

/// A Lucide icon, drawn.
///
/// Sized in points on the 24-unit grid and stroked proportionally, so it
/// keeps Lucide's weight at any size rather than growing a fat outline
/// when scaled up. Takes the foreground style from its context, exactly
/// as an SF Symbol does, so a row does not have to say the colour twice.
struct LucideIcon: View {
    let elements: [Lucide.Element]
    var size: CGFloat = 22

    var body: some View {
        Canvas { context, canvasSize in
            let scale = min(canvasSize.width, canvasSize.height) / 24
            let stroke = StrokeStyle(lineWidth: 2 * scale, lineCap: .round, lineJoin: .round)
            for element in elements {
                var path = Self.path(for: element)
                path = path.applying(CGAffineTransform(scaleX: scale, y: scale))
                context.stroke(path, with: .style(.foreground), style: stroke)
            }
        }
        .frame(width: size, height: size)
        // Decorative: the row's own text is the label, and a second
        // reading of the same thing is noise to anyone listening.
        .accessibilityHidden(true)
    }

    static func path(for element: Lucide.Element) -> Path {
        switch element {
        case .path(let d):
            return SVGPath.parse(d)
        case .circle(let x, let y, let r):
            return Path(ellipseIn: CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2))
        case .rect(let x, let y, let w, let h, let r):
            return Path(roundedRect: CGRect(x: x, y: y, width: w, height: h), cornerRadius: r)
        }
    }
}

/// A settings row: a Lucide icon and a title, aligned like `Label`.
///
/// `Label` cannot take an arbitrary view as its icon and keep the list's
/// alignment, so this is the same geometry written out — a fixed icon
/// column, which is what makes the titles line up down the screen.
struct LucideRow: View {
    let title: LocalizedStringKey
    let icon: [Lucide.Element]

    var body: some View {
        HStack(spacing: 12) {
            LucideIcon(elements: icon)
                .foregroundStyle(Color.accentColor)
                .frame(width: 26, alignment: .leading)
            Text(title)
                .foregroundStyle(.primary)
        }
    }
}
