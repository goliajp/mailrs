import SwiftUI

/// The web client's design tokens, ported value for value.
///
/// mailrs's web UI runs the gds `zinc-neutral` preset, whose light and
/// dark palettes are written out as explicit hex rather than derived —
/// so the two clients can hold the same colours rather than two
/// interpretations of the same intent. Names match the CSS custom
/// properties (`--gds-fg-muted` → `fgMuted`) so a change on either side
/// is greppable from the other.
///
/// Semantic, never literal: no view names a colour, it names a role.
struct Theme: Equatable, Sendable {
    let bg: Color
    let bgSecondary: Color
    let bgTertiary: Color
    let surface: Color
    let surfaceRaised: Color
    let fg: Color
    let fgSecondary: Color
    let fgMuted: Color
    let border: Color
    let borderStrong: Color
    let accent: Color
    let accentFg: Color
    let success: Color
    let warning: Color
    let danger: Color
    let info: Color

    static let light = Theme(
        bg: .hex(0xFAFAFA), bgSecondary: .hex(0xF4F4F5), bgTertiary: .hex(0xE4E4E7),
        surface: .hex(0xFFFFFF), surfaceRaised: .hex(0xFFFFFF),
        fg: .hex(0x09090B), fgSecondary: .hex(0x3F3F46), fgMuted: .hex(0x71717A),
        border: .hex(0xE4E4E7), borderStrong: .hex(0xD4D4D8),
        accent: .hex(0x3B7DDD), accentFg: .hex(0xFFFFFF),
        success: .hex(0x0CA678), warning: .hex(0xE67700),
        danger: .hex(0xE03131), info: .hex(0x3B7DDD)
    )

    static let dark = Theme(
        bg: .hex(0x09090B), bgSecondary: .hex(0x0A0A0A), bgTertiary: .hex(0x18181B),
        surface: .hex(0x18181B), surfaceRaised: .hex(0x27272A),
        fg: .hex(0xFAFAFA), fgSecondary: .hex(0xA1A1AA), fgMuted: .hex(0x71717A),
        border: .hex(0x27272A), borderStrong: .hex(0x3F3F46),
        accent: .hex(0x3B82F6), accentFg: .hex(0xFFFFFF),
        success: .hex(0x22C55E), warning: .hex(0xF59E0B),
        danger: .hex(0xEF4444), info: .hex(0x3B82F6)
    )

    static func of(_ scheme: ColorScheme) -> Theme {
        scheme == .dark ? .dark : .light
    }
}

extension Color {
    static func hex(_ value: UInt32) -> Color {
        Color(
            red: Double((value >> 16) & 0xFF) / 255,
            green: Double((value >> 8) & 0xFF) / 255,
            blue: Double(value & 0xFF) / 255
        )
    }
}

private struct ThemeKey: EnvironmentKey {
    static let defaultValue = Theme.light
}

extension EnvironmentValues {
    /// Read as `@Environment(\.theme)`. Set once at the root from the
    /// resolved colour scheme, so no view has to ask which mode it is
    /// in — asking is how one view ends up disagreeing with the next.
    var theme: Theme {
        get { self[ThemeKey.self] }
        set { self[ThemeKey.self] = newValue }
    }
}
