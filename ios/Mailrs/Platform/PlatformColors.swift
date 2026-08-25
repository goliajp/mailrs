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
            Color(.systemBackground)
        #endif
    }

    /// A card sitting on the grouped background.
    static var cardBackground: Color {
        #if os(macOS)
            Color(nsColor: .controlBackgroundColor)
        #else
            Color(.secondarySystemGroupedBackground)
        #endif
    }

    /// The recessed background a grouped list sits in. On the Mac the
    /// window's own background is that surface.
    static var groupedBackground: Color {
        #if os(macOS)
            Color(nsColor: .windowBackgroundColor)
        #else
            Color(.systemGroupedBackground)
        #endif
    }
}

#if os(macOS)
    import AppKit
#endif
