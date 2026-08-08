import SwiftUI

/// The app's mark, drawn rather than borrowed.
///
/// The same artwork as the icon (`web/public/icon.svg`): a red field,
/// a white envelope, a pink flap. It was a system `envelope.fill`
/// tinted with the accent, which made the sign-in screen blue while
/// the icon on the home screen was red — the first two things anyone
/// sees, disagreeing.
///
/// Drawn in SwiftUI rather than shipped as a second image so it stays
/// sharp at any size and cannot drift from the icon by being edited in
/// one place only: both come from the same geometry, written down
/// here and in the PIL script that renders the 1024px asset.
struct BrandMark: View {
    var size: CGFloat = 88

    /// The icon's gradient stops, top-left to bottom-right.
    private static let field = LinearGradient(
        colors: [Color.hex(0xEF4444), Color.hex(0xDC2626), Color.hex(0x991B1B)],
        startPoint: .topLeading,
        endPoint: .bottom
    )

    private static let flap = LinearGradient(
        colors: [Color.hex(0xFDE8E8), Color.hex(0xFCA5A5)],
        startPoint: .top,
        endPoint: .bottom
    )

    var body: some View {
        // The 512-unit box the SVG authors in, scaled to whatever size
        // the caller wants — so the proportions are the icon's and not
        // an approximation of them.
        let u = size / 512

        ZStack {
            RoundedRectangle(cornerRadius: 112 * u, style: .continuous)
                .fill(Self.field)

            RoundedRectangle(cornerRadius: 18 * u, style: .continuous)
                .fill(.white.opacity(0.95))
                .frame(width: 320 * u, height: 208 * u)
                .offset(y: (252 - 256) * u)

            Path { path in
                path.move(to: CGPoint(x: 96 * u, y: 168 * u))
                path.addLine(to: CGPoint(x: 116 * u, y: 148 * u))
                path.addLine(to: CGPoint(x: 396 * u, y: 148 * u))
                path.addLine(to: CGPoint(x: 416 * u, y: 168 * u))
                path.addLine(to: CGPoint(x: 256 * u, y: 284 * u))
                path.closeSubpath()
            }
            .fill(Self.flap)
            .frame(width: size, height: size)
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}
