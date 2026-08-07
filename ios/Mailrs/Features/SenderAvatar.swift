import SwiftUI

/// The colored-initial avatar, exactly the web's (`lib/avatar.ts`):
/// same 16-color palette, same 31-multiply hash over the address —
/// the same correspondent wears the same color on every client. The
/// hash runs in wrapping Int32 because that is what JS's `| 0` does,
/// and matching it is the point.
struct SenderAvatar: View {
    let sender: String
    var size: CGFloat = 36

    /// tailwind-500, in the web's array order.
    static let palette: [Color] = [
        Color(red: 0.937, green: 0.267, blue: 0.267), // red
        Color(red: 0.976, green: 0.451, blue: 0.086), // orange
        Color(red: 0.961, green: 0.620, blue: 0.043), // amber
        Color(red: 0.918, green: 0.702, blue: 0.031), // yellow
        Color(red: 0.518, green: 0.800, blue: 0.086), // lime
        Color(red: 0.133, green: 0.773, blue: 0.369), // green
        Color(red: 0.063, green: 0.725, blue: 0.506), // emerald
        Color(red: 0.078, green: 0.722, blue: 0.651), // teal
        Color(red: 0.024, green: 0.714, blue: 0.831), // cyan
        Color(red: 0.055, green: 0.647, blue: 0.914), // sky
        Color(red: 0.231, green: 0.510, blue: 0.965), // blue
        Color(red: 0.388, green: 0.400, blue: 0.945), // indigo
        Color(red: 0.545, green: 0.361, blue: 0.965), // violet
        Color(red: 0.659, green: 0.333, blue: 0.969), // purple
        Color(red: 0.851, green: 0.275, blue: 0.937), // fuchsia
        Color(red: 0.925, green: 0.282, blue: 0.600), // pink
    ]

    static func color(for sender: String) -> Color {
        let email = SenderName.extractEmail(sender)
        var hash: Int32 = 0
        for unit in email.utf16 {
            hash = hash &* 31 &+ Int32(unit)
        }
        return palette[Int(hash.magnitude) % palette.count]
    }

    static func initial(for sender: String) -> String {
        let name = SenderName.extractName(sender)
        guard let first = name.first else { return "?" }
        return String(first).uppercased()
    }

    var body: some View {
        ZStack {
            Circle()
                .fill(Self.color(for: sender).gradient)
            Text(Self.initial(for: sender))
                .font(.system(size: size * 0.42, weight: .semibold, design: .rounded))
                .foregroundStyle(.white)
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}
