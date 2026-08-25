import SwiftUI

/// Handing a URL to whatever opens it here.
///
/// SwiftUI's `openURL` environment value exists on both platforms and
/// is the right answer in a view. This is for the places that are not
/// views — a delegate, a decision handler — where there is no
/// environment to read.
enum OpenLink {
    static func open(_ url: URL) {
        #if os(macOS)
            NSWorkspace.shared.open(url)
        #else
            Task { @MainActor in _ = UIApplication.shared.open(url) }
        #endif
    }
}

#if os(macOS)
    import AppKit
#else
    import UIKit
#endif
