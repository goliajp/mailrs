import SwiftUI

/// Whether this screen gets the iPad design or the iPhone one.
///
/// **Not "is this an iPad".** An iPad in Slide Over is a tall narrow
/// column, and a three-pane mail client there is three cramped columns
/// — the phone layout is the right answer for it. A phone in landscape
/// is still a phone. The question is how much width this *scene* has,
/// which is what the horizontal size class answers, and it changes
/// while the app runs.
///
/// A pure function so the decision can be read and tested without a
/// tablet — the same reason Android keeps `Panes.twoPanes` apart from
/// the layout that follows from it.
enum PadLayout {
    /// `nil` is the size class before SwiftUI has resolved one, which
    /// happens for a frame or two at launch. Compact is the safe
    /// reading: a split view that appears and then collapses is worse
    /// than one that appears a frame late.
    static func splits(_ horizontalSizeClass: UserInterfaceSizeClass?) -> Bool {
        horizontalSizeClass == .regular
    }
}
