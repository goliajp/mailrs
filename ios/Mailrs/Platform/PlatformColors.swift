import SwiftUI

/// The system's own backgrounds, under one name.
///
/// `Color(.systemBackground)` and `Color(.systemGroupedBackground)` are
/// UIKit colours; AppKit's equivalents are named differently and there
/// is no grouped variant. Naming them here means the shared views ask
/// for *the page* and *the surface behind it* rather than for a
/// platform's spelling of them.
extension Color {
    /// What a sheet or a row sits on.
    static var pageBackground: Color {
        #if os(macOS)
            Color(nsColor: .textBackgroundColor)
        #else
            gds(light: 0xFFFFFF, dark: 0x09090B)
        #endif
    }

    /// A card sitting on the grouped background.
    static var cardBackground: Color {
        #if os(macOS)
            Color(nsColor: .controlBackgroundColor)
        #else
            gds(light: 0xFFFFFF, dark: 0x18181B)
        #endif
    }

    /// The recessed background a grouped list sits in. On the Mac the
    /// window's own background is that surface.
    static var groupedBackground: Color {
        #if os(macOS)
            Color(nsColor: .windowBackgroundColor)
        #else
            gds(light: 0xF4F4F5, dark: 0x09090B)
        #endif
    }

    #if !os(macOS)
        /// A gds zinc-neutral value for each appearance, so the app's own
        /// surfaces are the web client's rather than the system's: the
        /// system's dark grey cards and pure black pages are a different
        /// palette from the one `Theme` holds.
        static func gds(light: UInt32, dark: UInt32) -> Color {
            Color(UIColor { traits in
                if traits.userInterfaceStyle == .dark { return uiColor(dark) }
                return uiColor(light)
            })
        }

        private static func uiColor(_ value: UInt32) -> UIColor {
            UIColor(
                red: CGFloat((value >> 16) & 0xFF) / 255,
                green: CGFloat((value >> 8) & 0xFF) / 255,
                blue: CGFloat(value & 0xFF) / 255,
                alpha: 1)
        }
    #endif

    /// Orange for words that warn, dark enough to read in light mode.
    /// The system orange on white, or on a wash of itself, is about
    /// 2:1, and one of the lines drawn in it is a fraud warning.
    static var warningText: Color {
        #if os(macOS)
            Color.orange
        #else
            Color(UIColor { traits in
                if traits.userInterfaceStyle == .dark { return .systemOrange }
                return UIColor(red: 0xB4 / 255, green: 0x53 / 255, blue: 0x09 / 255, alpha: 1)
            })
        #endif
    }
}

#if os(macOS)
    import AppKit
#endif
