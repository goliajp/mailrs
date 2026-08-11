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
    /// Remote subresources are refused until the reader asks for them.
    var blockRemote: Bool = true
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
        // Phone numbers and postal addresses, tappable, the way every
        // other iOS app renders them. 42% of real message bodies carry
        // an address and 10% a phone number; without this they are
        // characters you can read and not act on.
        //
        // Not `.all`: that adds dates, and `NSDataDetector` returns the
        // single character 今 as one. A page of ordinary Japanese prose
        // would come back speckled with tappable nothing, and a date
        // has no unambiguous action anyway — see `BodyDetections`.
        config.dataDetectorTypes = [.phoneNumber, .address]

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator
        // Enabled, unlike before — but with no room to move vertically
        // at fit scale, so the thread behind it keeps the vertical pan
        // and the body keeps pinch and the horizontal one. A message
        // wider than the phone used to be simply cut off at the right
        // edge with no way to reach the rest.
        webView.scrollView.isScrollEnabled = true
        webView.scrollView.alwaysBounceVertical = false
        webView.scrollView.alwaysBounceHorizontal = false
        webView.scrollView.bounces = false
        // A long press on a link *fetches the target* to build the
        // peek. That undoes the remote-content block for anyone whose
        // thumb rests a moment too long, and for a tracking link it
        // confirms the open to the sender — the exact thing the rule
        // list above exists to prevent. Default is YES; mail is the
        // wrong document for it.
        webView.allowsLinkPreview = false
        webView.isOpaque = false
        // Clear, not white: the document paints its own paper, so the
        // card behind shows through for mail that follows the app.
        webView.backgroundColor = .clear
        webView.scrollView.backgroundColor = .clear
        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
        // A width this view has not been fitted against yet — the first
        // layout pass, a rotation, a split view. Re-fitting is cheap and
        // not re-fitting leaves the message at whatever scale it had
        // when it was one width narrower.
        let width = webView.bounds.width
        if width > 0, context.coordinator.fittedWidth != width {
            context.coordinator.fittedWidth = width
            context.coordinator.fit(webView)
        }
        // Every input that changes what is painted: the mail, the
        // appearance, and whether the reader has asked for the remote
        // half of it.
        guard context.coordinator.loadedHTML != html
                || context.coordinator.loadedDark != dark
                || context.coordinator.loadedBlocked != blockRemote else { return }
        context.coordinator.loadedHTML = html
        context.coordinator.loadedDark = dark
        context.coordinator.loadedBlocked = blockRemote
        let document = Self.document(for: html, dark: dark)
        guard blockRemote else {
            webView.configuration.userContentController.removeAllContentRuleLists()
            webView.loadHTMLString(document, baseURL: nil)
            return
        }
        // Compiled before the load, never after: a rule list installed
        // once the page is up arrives after the beacon has already
        // fired, which is the whole thing this prevents.
        Task { @MainActor in
            if let rules = await RemoteBlock.rules() {
                webView.configuration.userContentController.removeAllContentRuleLists()
                webView.configuration.userContentController.add(rules)
            }
            webView.loadHTMLString(document, baseURL: nil)
        }
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
        let palette = DocumentPalette.of(dark: dark)
        let scheme = palette.scheme
        let background = palette.background
        let text = palette.text
        let link = palette.link
        let rule = palette.rule
        let quote = palette.quote
        return """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width,initial-scale=1">
        <style>
          :root { color-scheme: \(scheme); }
          html, body { margin: 0; padding: 0; background: \(background); color: \(text); }
          /* No padding of our own. The card around this view already
             insets by 12, so a second inset cost the message 24pt of a
             358pt screen — and a newsletter laid out at 600px pays for
             that twice, once in the fit scale and once in the reading. */
          body { font: 15px/1.6 -apple-system, 'Hiragino Sans', sans-serif;
                 word-wrap: break-word; overflow-wrap: break-word; }
          /* `!important`, because senders put the width in an attribute
             (`<img width="600">`) or an inline style, and both beat a
             plain rule. Without it a 600px logo pushes the document out
             to 600px and the whole message scales down to fit it. */
          img { max-width: 100% !important; height: auto !important; }
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
        var loadedBlocked = true
        /// The width the current scale was computed for.
        var fittedWidth: CGFloat = 0

        init(_ parent: MessageBodyView) {
            self.parent = parent
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            fit(webView)
        }

        /// Measure what the message wants, then scale it to what it has.
        ///
        /// Split out of `didFinish` so it can run again: the first
        /// measurement happens while SwiftUI may still be laying the
        /// card out, and a web view whose bounds are still zero yields
        /// no usable scale. Reported from the phone as wide mail
        /// arriving unscaled — needing a pinch and a sideways drag —
        /// which is exactly what a fit computed against a width of 0
        /// looks like.
        func fit(_ webView: WKWebView, attempt: Int = 0) {
            // Several answers to "how wide is this", and the widest one
            // is the truth. `documentElement.scrollWidth` alone misses a
            // wide child inside a container that does not itself
            // overflow.
            let js = """
            (function () {
              var d = document.documentElement, b = document.body;
              var w = Math.max(d.scrollWidth, d.offsetWidth,
                               b ? b.scrollWidth : 0, b ? b.offsetWidth : 0);
              var h = Math.max(d.scrollHeight, b ? b.scrollHeight : 0);
              return [w, h];
            })()
            """
            webView.evaluateJavaScript(js) { result, _ in
                guard let pair = result as? [Double], pair.count == 2 else { return }
                let contentWidth = pair[0]
                let contentHeight = pair[1]
                let hostWidth = Double(webView.bounds.width)
                // Laid out later than this ran. Ask again rather than
                // fit to nothing — bounded, so a view that never gets a
                // width does not spin.
                guard hostWidth > 0 else {
                    guard attempt < 5 else { return }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
                        self.fit(webView, attempt: attempt + 1)
                    }
                    return
                }
                let scale = FitToWidth.scale(contentWidth: contentWidth, hostWidth: hostWidth)
                // Fit is where it starts, not where it is stuck. Both
                // bounds used to be the fit scale, which pinned the
                // zoom: a newsletter laid out at 1080px arrived at 0.33
                // — legible only to someone who could enlarge it, and
                // nobody could. Up to 1:1, or four times the fit,
                // whichever is further.
                // `pageZoom`, not `scrollView.zoomScale`.
                //
                // The scale was right all along — a 782px newsletter
                // against a 358pt card measures 0.46 — and setting it on
                // the scroll view did nothing, because WKWebView owns
                // its own zoom and overrides what is written there. Wide
                // mail therefore arrived at 1:1 needing a pinch and a
                // sideways drag, which is how it was reported from the
                // phone. `pageZoom` is the supported control, and it
                // reflows rather than scaling a bitmap, so the text
                // stays sharp at 0.46.
                guard scale < 1 else {
                    self.height(of: webView, scale: 1, measured: contentHeight)
                    return
                }
                webView.pageZoom = CGFloat(scale)
                // Re-measured after the zoom: the layout viewport is now
                // wider in CSS pixels, so the height from before it is
                // the wrong document's height.
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
                    webView.evaluateJavaScript(js) { again, _ in
                        let zoomed = (again as? [Double])?.last ?? contentHeight
                        self.height(of: webView, scale: scale, measured: zoomed)
                    }
                }
            }
        }

        /// The card's height: what the document needs, at the zoom it
        /// is being shown at.
        ///
        /// Animated, because this lands after first paint — the card
        /// grows from its placeholder rather than the whole thread
        /// lurching by the body's full height in one frame.
        func height(of webView: WKWebView, scale: Double, measured: Double) {
            withAnimation(.easeOut(duration: 0.2)) {
                self.parent.height = CGFloat(measured * scale)
            }
        }

        /// A message body may not navigate — see `WebNavigation`.
        ///
        /// The **async** spelling, which is the one the SDK advertises
        /// (`WK_SWIFT_ASYNC(3)`), and the reason matters: the
        /// completion-handler form takes a `WK_SWIFT_UI_ACTOR` closure,
        /// i.e. `@MainActor`. Written without that attribute — as this
        /// file did since it was created — the method does not match the
        /// **optional** protocol requirement, so no `@objc` is inferred,
        /// WebKit never sees it, and nothing is reported: no error, no
        /// warning at the call site, and a delegate that silently does
        /// not exist. Link taps were never being intercepted either; it
        /// took a `<meta http-equiv="refresh">` in a fixture, and an
        /// NSLog that printed nothing at all, to notice.
        func webView(
            _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
        ) async -> WKNavigationActionPolicy {
            let url = navigationAction.request.url
            switch WebNavigation.decide(
                isLinkActivation: navigationAction.navigationType == .linkActivated, url: url
            ) {
            case .allow:
                return .allow
            case .openExternally:
                // `open(_:)` has an async form now, and in an async
                // context Swift picks it; the result is not interesting
                // — Safari either takes the URL or it was not openable.
                if let url { _ = await UIApplication.shared.open(url) }
                return .cancel
            case .refuse:
                return .cancel
            }
        }
    }
}

/// The content-blocking rule the message bodies load under.
///
/// A rule list rather than rewriting `src` attributes: it refuses at
/// the network layer, so it also covers the pixel hiding in a CSS
/// background — which attribute rewriting misses, and which is where
/// a sender puts one that does not want to be found. Only http and
/// https are refused, so `data:` images the message carries itself
/// still render.
@MainActor
enum RemoteBlock {
    private static let source = """
    [{"trigger":{"url-filter":"^https?://",
      "resource-type":["image","style-sheet","font","media","raw","script","svg-document"]},
      "action":{"type":"block"}}]
    """

    /// Compiled once — the compile is a disk-backed operation, and a
    /// per-message one would put it between the open and the paint.
    private static var compiled: WKContentRuleList?

    static func rules() async -> WKContentRuleList? {
        if let compiled { return compiled }
        let store = WKContentRuleListStore.default()
        let list = try? await store?.compileContentRuleList(
            forIdentifier: "mailrs-block-remote", encodedContentRuleList: source
        )
        compiled = list
        return list
    }
}

/// The colours the message document is written with.
///
/// One value chosen once rather than six conditionals in a row: the
/// two sets are two designs, and reading them side by side is how you
/// see that the dark one is transparent — the card behind it is the
/// paper, so the body and its rounded corners stay one surface.
private struct DocumentPalette {
    let scheme: String
    let background: String
    let text: String
    let link: String
    let rule: String
    let quote: String

    static let light = DocumentPalette(
        scheme: "light", background: "#fff", text: "#1a1a1a",
        link: "#2563eb", rule: "#d4d4d8", quote: "#71717a"
    )

    static let dark = DocumentPalette(
        scheme: "dark", background: "transparent", text: "#e6e6ea",
        link: "#6ea8fe", rule: "#48484a", quote: "#a1a1aa"
    )

    static func of(dark: Bool) -> DocumentPalette {
        if dark { return .dark }
        return .light
    }
}
