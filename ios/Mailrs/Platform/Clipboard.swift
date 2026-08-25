import SwiftUI

/// Putting text on the clipboard, on either platform.
///
/// `UIPasteboard` and `NSPasteboard` do the same thing under different
/// names, and the difference is not worth spreading through four call
/// sites — nor is `#if os(macOS)` at each of them, which is the same
/// duplication written four times instead of once.
enum Clipboard {
    static func put(_ text: String) {
        #if os(macOS)
            // Cleared first: AppKit's pasteboard appends to whatever is
            // on it unless told otherwise, and a copy that pastes the
            // last two things is worse than one that fails.
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        #else
            UIPasteboard.general.string = text
        #endif
    }
}

#if os(macOS)
    import AppKit
#else
    import UIKit
#endif
