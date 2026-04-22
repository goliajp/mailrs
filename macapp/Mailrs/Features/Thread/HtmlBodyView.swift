import SwiftUI
import WebKit

#if os(iOS)
import UIKit
typealias PlatformViewRepresentable = UIViewRepresentable
#else
import AppKit
typealias PlatformViewRepresentable = NSViewRepresentable
#endif

/// Sandboxed HTML email renderer. JavaScript is disabled, data store is
/// non-persistent (no cookies leak between messages), and remote images are
/// blocked by default until the user opts in.
struct HtmlBodyView: View {
    let html: String
    @State private var loadRemoteImages: Bool = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if containsRemoteImages && !loadRemoteImages {
                Button {
                    loadRemoteImages = true
                } label: {
                    Label("加载图片", systemImage: "photo")
                        .font(.caption)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
            WebBody(html: html, allowRemoteImages: loadRemoteImages)
        }
    }

    private var containsRemoteImages: Bool {
        html.range(of: #"<img\b[^>]*\bsrc="https?:"#, options: .regularExpression) != nil
    }
}

private struct WebBody: PlatformViewRepresentable {
    let html: String
    let allowRemoteImages: Bool

    #if os(iOS)
    func makeUIView(context: Context) -> DynamicHeightWebView { Self.make(context: context) }
    func updateUIView(_ view: DynamicHeightWebView, context: Context) { Self.update(view, html: html, allowRemoteImages: allowRemoteImages) }
    #else
    func makeNSView(context: Context) -> DynamicHeightWebView { Self.make(context: context) }
    func updateNSView(_ view: DynamicHeightWebView, context: Context) { Self.update(view, html: html, allowRemoteImages: allowRemoteImages) }
    #endif

    private static func make(context: Context) -> DynamicHeightWebView {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.websiteDataStore = .nonPersistent()
        let view = DynamicHeightWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        #if os(iOS)
        view.isOpaque = false
        view.backgroundColor = .clear
        view.scrollView.isScrollEnabled = false
        view.scrollView.bounces = false
        #else
        view.setValue(false, forKey: "drawsBackground")
        #endif
        return view
    }

    private static func update(_ view: DynamicHeightWebView, html: String, allowRemoteImages: Bool) {
        let current = view.lastLoadedKey
        let key = "\(allowRemoteImages)\n\(html.hashValue)"
        guard current != key else { return }
        view.lastLoadedKey = key
        let wrapped = Self.wrap(html: html, allowRemoteImages: allowRemoteImages)
        view.loadHTMLString(wrapped, baseURL: nil)
    }

    private static func wrap(html: String, allowRemoteImages: Bool) -> String {
        let cspImg = allowRemoteImages ? "img-src * data: blob:" : "img-src 'self' data: blob: cid:"
        return """
        <!doctype html>
        <html>
        <head>
        <meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <meta http-equiv="Content-Security-Policy" content="default-src 'self' data: blob:; script-src 'none'; style-src 'unsafe-inline'; \(cspImg); font-src *;">
        <base target="_blank">
        <style>
          :root { color-scheme: light dark; }
          html, body { margin: 0; padding: 0; background: transparent; color: inherit; }
          body {
            font: -apple-system-body;
            padding: 12px 16px;
            word-wrap: break-word;
            overflow-wrap: anywhere;
          }
          img { max-width: 100% !important; height: auto !important; }
          table { max-width: 100% !important; }
          blockquote { border-left: 3px solid rgba(128,128,128,0.4); margin-left: 0; padding-left: 8px; color: rgba(128,128,128,0.9); }
          a { color: -apple-system-blue; }
          pre, code { white-space: pre-wrap; word-break: break-word; }
          @media (prefers-color-scheme: dark) {
            body { color: #eee; }
            blockquote { color: #aaa; }
          }
        </style>
        </head>
        <body>\(html)</body>
        </html>
        """
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject, WKNavigationDelegate {
        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                     decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            // Allow the initial main-frame HTML load; open everything else externally.
            if navigationAction.navigationType == .other, navigationAction.request.url?.scheme == "about" {
                decisionHandler(.allow)
                return
            }
            if let url = navigationAction.request.url,
               (navigationAction.navigationType == .linkActivated ||
                navigationAction.targetFrame == nil) {
                #if os(iOS)
                UIApplication.shared.open(url)
                #else
                NSWorkspace.shared.open(url)
                #endif
                decisionHandler(.cancel)
                return
            }
            decisionHandler(.allow)
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            guard let view = webView as? DynamicHeightWebView else { return }
            view.measureHeight()
        }
    }
}

/// WKWebView that reports its content height up as intrinsicContentSize.
final class DynamicHeightWebView: WKWebView {
    var contentHeight: CGFloat = 200 { didSet { invalidateIntrinsicContentSize() } }
    var lastLoadedKey: String?

    override var intrinsicContentSize: CGSize {
        #if os(iOS)
        return CGSize(width: UIView.noIntrinsicMetric, height: contentHeight)
        #else
        return CGSize(width: NSView.noIntrinsicMetric, height: contentHeight)
        #endif
    }

    func measureHeight() {
        evaluateJavaScript("document.body.scrollHeight") { [weak self] value, _ in
            guard let self, let n = value as? CGFloat else { return }
            DispatchQueue.main.async {
                if abs(n - self.contentHeight) > 1 {
                    self.contentHeight = n
                }
            }
        }
    }
}
