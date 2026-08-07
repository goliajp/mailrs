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
    @Environment(\.colorScheme) private var colorScheme

    /// Dark paper only for mail that declares no colours of its own —
    /// see `MailAppearance`.
    private var dark: Bool {
        colorScheme == .dark && MailAppearance.followsAppTheme(html: html)
    }

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
        // Clear, not white: the document paints its own paper, so the
        // card behind shows through for mail that follows the app.
        webView.backgroundColor = .clear
        webView.scrollView.backgroundColor = .clear
        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
        // The scheme is part of what was loaded: switching appearance
        // with the thread open has to repaint, and comparing only the
        // HTML left the old paper in place.
        guard context.coordinator.loadedHTML != html
                || context.coordinator.loadedDark != dark else { return }
        context.coordinator.loadedHTML = html
        context.coordinator.loadedDark = dark
        webView.loadHTMLString(Self.document(for: html, dark: dark), baseURL: nil)
    }

    /// The message wrapped in a document that pins the things email
    /// cannot be trusted to get right.
    ///
    /// Mail that declares its own colours is rendered on white paper
    /// whatever the phone's appearance — honouring dark for a message
    /// that sets black text produces black on black, which is worse
    /// than a bright rectangle. Mail that declares none follows the
    /// app, because for a paragraph and a link the bright rectangle is
    /// the only thing wrong with the screen.
    private static func document(for html: String, dark: Bool) -> String {
        let scheme = dark ? "dark" : "light"
        // Transparent when following the app: the card behind is the
        // paper, so the body and its rounded corners stay one surface.
        let background = dark ? "transparent" : "#fff"
        let text = dark ? "#e6e6ea" : "#1a1a1a"
        let link = dark ? "#6ea8fe" : "#2563eb"
        let rule = dark ? "#48484a" : "#d4d4d8"
        let quote = dark ? "#a1a1aa" : "#71717a"
        return """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width,initial-scale=1">
        <style>
          :root { color-scheme: \(scheme); }
          html, body { margin: 0; padding: 0; background: \(background); color: \(text); }
          body { padding: 12px; font: 15px/1.6 -apple-system, 'Hiragino Sans', sans-serif;
                 word-wrap: break-word; overflow-wrap: break-word; }
          img { max-width: 100%; height: auto; }
          a { color: \(link); }
          pre { overflow-x: auto; }
          blockquote { border-left: 3px solid \(rule); padding-left: 12px;
                       margin: 8px 0; color: \(quote); }
        </style></head><body>\(html)</body></html>
        """
    }

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate {
        private let parent: MessageBodyView
        var loadedHTML: String?
        var loadedDark = false

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
                //
                // Animated, because this lands after first paint: the
                // card grows from its placeholder to the measured height,
                // and without the animation the whole thread lurches by
                // the body's full height in one frame — the jump Apple
                // Mail never shows.
                withAnimation(.easeOut(duration: 0.2)) {
                    self.parent.height = CGFloat(contentHeight * scale)
                }
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
