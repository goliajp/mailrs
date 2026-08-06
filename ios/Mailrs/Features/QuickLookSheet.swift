import QuickLook
import SwiftUI

/// Quick Look, as a sheet.
///
/// SwiftUI's `.quickLookPreview` modifier is macOS-only, so on iOS this
/// is the UIKit controller wrapped up. Worth the wrapper rather than
/// rendering attachments here: Quick Look already handles every format a
/// phone can display, and hands the rest to the share sheet.
struct QuickLookSheet: UIViewControllerRepresentable {
    let url: URL
    let onDone: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(url: url, onDone: onDone) }

    func makeUIViewController(context: Context) -> UINavigationController {
        let controller = QLPreviewController()
        controller.dataSource = context.coordinator
        // The Done button is added here, not inherited. Wrapping
        // QLPreviewController in a navigation controller gives it a bar
        // and nothing to put in it — the preview opened with Quick Look's
        // own overlay (Markup, Share) and no way back except dragging the
        // sheet down, which is not something a button-shaped affordance
        // should be left to.
        controller.navigationItem.leftBarButtonItem = UIBarButtonItem(
            systemItem: .done,
            primaryAction: UIAction { _ in context.coordinator.onDone() }
        )
        return UINavigationController(rootViewController: controller)
    }

    func updateUIViewController(_ controller: UINavigationController, context: Context) {}

    final class Coordinator: NSObject, QLPreviewControllerDataSource {
        private let url: URL
        let onDone: () -> Void

        init(url: URL, onDone: @escaping () -> Void) {
            self.url = url
            self.onDone = onDone
        }

        func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }

        func previewController(
            _ controller: QLPreviewController, previewItemAt index: Int
        ) -> QLPreviewItem {
            url as NSURL
        }
    }
}
