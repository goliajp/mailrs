import SwiftUI
import WebKit

/// An email body, scaled to fit the width it is given.
///
/// A `WKWebView` rather than parsing the HTML into SwiftUI views: mail is
/// arbitrary HTML with tables and absolute widths, and anything that
/// reinterprets it shows something the sender did not write.
///
/// The scale comes from `FitToWidth`, the same rule the web client uses,
/// applied through a viewport meta the page never gets to see: senders
/// set their own `<meta viewport>` and would otherwise fight it.
struct MessageBodyView: UIViewRepresentable {
    let html: String
    @Binding var height: CGFloat

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        // Nothing in an email needs to open a window, autoplay, or pick
        // up an existing login: a fresh, non-persistent store means a
        // tracking cookie set by one message cannot be read by the next.
        config.websiteDataStore = .nonPersistent()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.suppressesIncrementalRendering = true

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator
        webView.scrollView.isScrollEnabled = false
        webView.scrollView.bounces = false
        webView.isOpaque = false
        webView.backgroundColor = .white
        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
        guard context.coordinator.loadedHTML != html else { return }
        context.coordinator.loadedHTML = html
        webView.loadHTMLString(Self.document(for: html), baseURL: nil)
    }

    /// The message wrapped in a document that pins the things email
    /// cannot be trusted to get right.
    ///
    /// Light-mode regardless of the phone's appearance: HTML mail is
    /// authored against a white background and almost none of it
    /// supports dark mode, so honouring the system setting produces
    /// black text on black far more often than it produces dark mode.
    private static func document(for html: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width,initial-scale=1">
        <style>
          :root { color-scheme: light; }
          html, body { margin: 0; padding: 0; background: #fff; color: #1a1a1a; }
          body { padding: 12px; font: 15px/1.6 -apple-system, 'Hiragino Sans', sans-serif;
                 word-wrap: break-word; overflow-wrap: break-word; }
          img { max-width: 100%; height: auto; }
          a { color: #2563eb; }
          pre { overflow-x: auto; }
          blockquote { border-left: 3px solid #d4d4d8; padding-left: 12px;
                       margin: 8px 0; color: #71717a; }
        </style></head><body>\(html)</body></html>
        """
    }

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate {
        private let parent: MessageBodyView
        var loadedHTML: String?

        init(_ parent: MessageBodyView) {
            self.parent = parent
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            // Measure the width the content wants, then scale to the width
            // it has — `scrollWidth` is the only thing that knows about a
            // `<table width="700">` nested six levels down.
            webView.evaluateJavaScript("[document.documentElement.scrollWidth, document.body.scrollHeight]") { result, _ in
                guard let pair = result as? [Double], pair.count == 2 else { return }
                let contentWidth = pair[0]
                let contentHeight = pair[1]
                let hostWidth = Double(webView.bounds.width)
                let scale = FitToWidth.scale(contentWidth: contentWidth, hostWidth: hostWidth)
                webView.scrollView.minimumZoomScale = scale
                webView.scrollView.maximumZoomScale = scale
                webView.scrollView.zoomScale = scale
                // A transform does not change layout, so the height the
                // view needs is the scaled one — without this the row
                // keeps the unscaled height and leaves a blank band.
                self.parent.height = CGFloat(contentHeight * scale)
            }
        }

        /// Links open in Safari; nothing navigates inside the message.
        func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction,
            decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
        ) {
            if navigationAction.navigationType == .linkActivated,
               let url = navigationAction.request.url {
                UIApplication.shared.open(url)
                decisionHandler(.cancel)
                return
            }
            decisionHandler(.allow)
        }
    }
}
