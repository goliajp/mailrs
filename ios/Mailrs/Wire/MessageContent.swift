import Foundation

/// What a message body is made of, and what to do about a navigation
/// out of one.
///
/// Both rules were written against the mail actually in this mailbox —
/// 31,501 messages, a 900-message sample classified by structure — not
/// against what mail is supposed to look like.
enum MessageContent {
    /// What the card should draw.
    ///
    /// Nine messages in the sample had **no body at all**: their whole
    /// content was a zip, or a delivery report, or a signature part with
    /// nothing signed that this client can read. They rendered as an
    /// empty card — a message you opened that shows nothing, with no
    /// word about why.
    enum Body: Equatable {
        case html(String)
        case text(String)
        case empty
    }

    static func body(html: String?, text: String?) -> Body {
        if let html, !html.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return .html(html)
        }
        if let text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return .text(text)
        }
        return .empty
    }

    /// The parts worth listing as files.
    ///
    /// An S/MIME signature is not a document. It rides on every part of
    /// a signed message — 16 of 900 here — and listing `smime.p7s` as an
    /// attachment offers the reader a file that does nothing when
    /// tapped. It is dropped from the list and *nothing is claimed about
    /// it*: this client does not verify signatures, so a "Signed" badge
    /// would assert something nobody checked.
    /// The index travels with the attachment because it is the only
    /// handle the server takes — the wire carries no id, so a part is
    /// fetched by its position in the array as sent. Filtering a plain
    /// list and re-enumerating it would shift every index after the one
    /// removed, and download the wrong file.
    ///
    /// `inlined` are the parts the body draws itself — see
    /// `InlineImages`. Listing them again offers the reader a file that
    /// is already on the screen, which Apple Mail does not do either.
    static func listable(
        _ attachments: [Wire.Attachment], inlined: [Int] = []
    ) -> [(index: Int, attachment: Wire.Attachment)] {
        let shown = Set(inlined)
        return attachments.enumerated()
            .filter { !isSignature($0.element) && !shown.contains($0.offset) }
            .map { (index: $0.offset, attachment: $0.element) }
    }

    static func isSignature(_ attachment: Wire.Attachment) -> Bool {
        let type = attachment.contentType.lowercased()
        if type.contains("pkcs7-signature") || type.contains("pkcs7-mime") { return true }
        if type == "application/x-pkcs7-signature" { return true }
        let name = attachment.filename.lowercased()
        return name == "smime.p7s" || name.hasSuffix(".p7s")
    }
}

/// Whether a message body may navigate.
///
/// The rendered document is a `WKWebView`, and a web view navigates. The
/// policy shipped until now refused link taps and **allowed everything
/// else**, which is not a policy: a `<form action="https://…"
/// method="post">` submits inside the card, and a `<meta
/// http-equiv="refresh">` walks the reader off the message on its own.
/// Four messages in the 900 carried a form; JavaScript is off, which
/// stops none of that — a form post needs no script.
///
/// So the rule is inverted. The only navigation a message body is
/// allowed is the one that put it there.
enum WebNavigation {
    enum Decision: Equatable {
        /// The initial `loadHTMLString`, and nothing else.
        case allow
        /// A tapped link: Safari takes it. That is also where the only
        /// phishing protection on the platform lives — there is no
        /// fraudulent-content API in the SDK, so handing the URL over is
        /// the whole of what an app can do.
        case openExternally
        /// A form post, a redirect, a meta refresh: refused.
        case refuse
    }

    /// `isLinkActivation` is the navigation type; `url` is what it wants
    /// to load. `loadHTMLString(_:baseURL: nil)` navigates to
    /// `about:blank`, which is the one thing let through.
    static func decide(isLinkActivation: Bool, url: URL?) -> Decision {
        if isLinkActivation { return .openExternally }
        guard let url else { return .allow }
        if url.scheme == nil || url.scheme?.lowercased() == "about" { return .allow }
        return .refuse
    }
}
