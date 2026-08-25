import SwiftUI

#if os(macOS)
    import AppKit

    typealias PlatformImage = NSImage

    extension Image {
        init(platformImage: PlatformImage) { self.init(nsImage: platformImage) }
    }
#else
    import UIKit

    typealias PlatformImage = UIImage

    extension Image {
        init(platformImage: PlatformImage) { self.init(uiImage: platformImage) }
    }
#endif
