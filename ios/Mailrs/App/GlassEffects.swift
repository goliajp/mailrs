import SwiftUI

/// iOS 26's Liquid Glass, where the platform has it.
///
/// Used only on things that float *over* content — a toast, a banner,
/// a prompt. Glass is a statement about layering, so putting it on the
/// content itself says the mail is hovering above something, which is
/// both untrue and unreadable. The deployment target is iOS 18, so
/// every use falls back to the material it replaced rather than to
/// nothing.
extension View {
    @ViewBuilder
    func floatingGlass(in shape: some Shape, tint: Color? = nil) -> some View {
        if #available(iOS 26.0, *) {
            if let tint {
                glassEffect(.regular.tint(tint), in: shape)
            } else {
                glassEffect(.regular, in: shape)
            }
        } else {
            background(.ultraThinMaterial, in: shape)
        }
    }

    /// The soft fade a scroll view's content gets under a bar, so rows
    /// dissolve into the chrome instead of sliding under a hard edge.
    @ViewBuilder
    func softScrollEdges(_ edges: Edge.Set = .all) -> some View {
        if #available(iOS 26.0, *) {
            scrollEdgeEffectStyle(.soft, for: edges)
        } else {
            self
        }
    }
}
