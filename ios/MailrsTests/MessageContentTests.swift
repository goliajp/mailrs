import Foundation
import Testing

@testable import Mailrs

/// The rules that came out of reading the mailbox rather than the spec.
struct MessageContentTests {
    private func attachment(_ name: String, _ type: String) -> Wire.Attachment {
        Wire.Attachment(filename: name, contentType: type, size: 10, contentId: nil)
    }

    @Test func htmlWinsWhenBothArePresent() {
        #expect(MessageContent.body(html: "<p>hi</p>", text: "hi") == .html("<p>hi</p>"))
    }

    @Test func textIsUsedWhenThereIsNoHtml() {
        #expect(MessageContent.body(html: nil, text: "hi") == .text("hi"))
        #expect(MessageContent.body(html: "", text: "hi") == .text("hi"))
    }

    /// Whitespace is not content. A body of "\n\n" rendered as a blank
    /// card just the same as no body at all.
    @Test func whitespaceIsNotABody() {
        #expect(MessageContent.body(html: "  \n ", text: "\n\n") == .empty)
        #expect(MessageContent.body(html: nil, text: nil) == .empty)
    }

    /// An S/MIME signature is not a file anyone wants to tap.
    @Test func signaturesAreNotListedAsFiles() {
        #expect(MessageContent.isSignature(attachment("smime.p7s", "application/pkcs7-signature")))
        #expect(MessageContent.isSignature(attachment("smime.p7s", "application/x-pkcs7-signature")))
        // Some senders label it as nothing useful; the name still says.
        #expect(MessageContent.isSignature(attachment("smime.p7s", "application/octet-stream")))
        #expect(!MessageContent.isSignature(attachment("invoice.pdf", "application/pdf")))
    }

    /// The index is the only handle the server takes, so it has to
    /// survive the filtering — this is the assertion that a dropped
    /// signature does not make every later attachment download the
    /// wrong bytes.
    @Test func filteringKeepsTheServersIndices() {
        let files = MessageContent.listable([
            attachment("invoice.pdf", "application/pdf"),
            attachment("smime.p7s", "application/pkcs7-signature"),
            attachment("photo.png", "image/png"),
        ])
        #expect(files.map(\.index) == [0, 2])
        #expect(files.map(\.attachment.filename) == ["invoice.pdf", "photo.png"])
    }
}

/// The navigation policy, which is the security-relevant half.
struct WebNavigationTests {
    @Test func theInitialDocumentLoadIsAllowed() {
        #expect(WebNavigation.decide(isLinkActivation: false, url: nil) == .allow)
        #expect(WebNavigation.decide(isLinkActivation: false,
                                     url: URL(string: "about:blank")) == .allow)
    }

    @Test func aTappedLinkGoesToSafari() {
        #expect(WebNavigation.decide(isLinkActivation: true,
                                     url: URL(string: "https://example.com")) == .openExternally)
    }

    /// The one that was open: a form post needs no JavaScript, and
    /// JavaScript being off stopped nothing.
    @Test func aFormPostIsRefused() {
        #expect(WebNavigation.decide(isLinkActivation: false,
                                     url: URL(string: "https://evil.example/collect")) == .refuse)
    }

    /// A meta refresh arrives as an ordinary navigation with a real URL,
    /// and walks the reader off the message without a tap.
    @Test func aMetaRefreshIsRefused() {
        #expect(WebNavigation.decide(isLinkActivation: false,
                                     url: URL(string: "http://tracker.example/r?id=1")) == .refuse)
    }

    /// And so is anything wearing another scheme.
    @Test func otherSchemesAreRefused() {
        for raw in ["file:///etc/passwd", "mailrs://x", "javascript:void(0)", "data:text/html,<b>x"] {
            #expect(WebNavigation.decide(isLinkActivation: false, url: URL(string: raw)) == .refuse,
                    "expected \(raw) to be refused")
        }
    }
}
