import SwiftUI

/// Field and navigation modifiers that exist on one platform only.
///
/// `keyboardType`, `textInputAutocapitalization` and
/// `navigationBarTitleDisplayMode` are iOS-only, and the views that
/// use them are shared with the Mac. Wrapping them here keeps the
/// `#if` in one place instead of at every field — and, more to the
/// point, keeps the shared view readable as a description of the form
/// rather than of the platforms.
extension View {
    /// A hint about which keyboard to raise. The Mac has one keyboard.
    @ViewBuilder func mailKeyboard() -> some View {
        #if os(iOS)
            keyboardType(.emailAddress).textInputAutocapitalization(.never)
        #else
            self
        #endif
    }

    /// Never capitalise. On the Mac nothing does.
    @ViewBuilder func neverCapitalised() -> some View {
        #if os(iOS)
            textInputAutocapitalization(.never)
        #else
            self
        #endif
    }

    /// Digits only. The Mac has one keyboard.
    @ViewBuilder func numberKeyboard() -> some View {
        #if os(iOS)
            keyboardType(.numberPad)
        #else
            self
        #endif
    }

    /// A hint for typing a server name or an address.
    @ViewBuilder func urlKeyboard() -> some View {
        #if os(iOS)
            keyboardType(.URL).textInputAutocapitalization(.never)
        #else
            self
        #endif
    }

    /// An inline title bar. The Mac's window title is not a bar.
    @ViewBuilder func inlineTitle() -> some View {
        #if os(iOS)
            navigationBarTitleDisplayMode(.inline)
        #else
            self
        #endif
    }
}

/// Toolbar placements that name the *role* rather than the edge.
///
/// `topBarTrailing` and `bottomBar` are iOS-only, and a Mac window has
/// neither — it has one unified toolbar. Saying "the primary action"
/// lets each platform put it where that platform puts primary actions,
/// which is the whole difference between a Mac app and an iOS app in a
/// window.
extension ToolbarItemPlacement {
    static var primaryActions: ToolbarItemPlacement {
        #if os(macOS)
            .primaryAction
        #else
            .topBarTrailing
        #endif
    }

    /// Where a screen's own secondary controls go: a bottom bar on a
    /// phone, and the toolbar on a Mac, which has no bottom bar.
    /// The screen's own leading control — Cancel, Close, Back.
    static var leadingAction: ToolbarItemPlacement {
        #if os(macOS)
            .cancellationAction
        #else
            .topBarLeading
        #endif
    }

    static var screenActions: ToolbarItemPlacement {
        #if os(macOS)
            .automatic
        #else
            .bottomBar
        #endif
    }
}
