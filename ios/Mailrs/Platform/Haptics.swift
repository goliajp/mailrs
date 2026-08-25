import SwiftUI

/// A tap on the wrist. The Mac has no Taptic Engine, and silence is
/// the correct behaviour there rather than a sound substituted for it.
enum Haptics {
    enum Kind { case success, warning, error }

    static func play(_ kind: Kind) {
        #if os(iOS)
            let generator = UINotificationFeedbackGenerator()
            switch kind {
            case .success: generator.notificationOccurred(.success)
            case .warning: generator.notificationOccurred(.warning)
            case .error: generator.notificationOccurred(.error)
            }
        #endif
    }
}

#if os(iOS)
    import UIKit
#endif
