import Testing

@testable import Mailrs

struct RemoteContentTests {
    @Test func spotsImagesAndBackgrounds() {
        #expect(RemoteContent.hasRemoteReferences(html: "<img src=\"https://x/p.gif\">"))
        #expect(RemoteContent.hasRemoteReferences(html: "<img src='http://x/p.gif'>"))
        #expect(RemoteContent.hasRemoteReferences(html: "<td background=\"http://x/b.png\">"))
    }

    /// A pixel hidden in CSS reports exactly like one in an `img`, and
    /// rewriting `src` attributes alone would have missed it.
    @Test func spotsPixelsHidingInStyles() {
        #expect(RemoteContent.hasRemoteReferences(html: "<div style=\"background:url(https://x/p.gif)\">"))
        #expect(RemoteContent.hasRemoteReferences(html: "<style>body{background:url('//x/p.gif')}</style>"))
    }

    /// Protocol-relative URLs are remote too — they inherit https here.
    @Test func spotsProtocolRelativeReferences() {
        #expect(RemoteContent.hasRemoteReferences(html: "<img src=\"//cdn.example.com/p.gif\">"))
    }

    /// Mail that carries its own images has nothing to load and must
    /// not wear a banner about it.
    @Test func inlineAndPlainMailAreLocal() {
        #expect(!RemoteContent.hasRemoteReferences(html: "<p>Hello</p>"))
        #expect(!RemoteContent.hasRemoteReferences(html: "<img src=\"data:image/png;base64,iVBOR\">"))
        #expect(!RemoteContent.hasRemoteReferences(html: "<img src=\"cid:logo@example\">"))
    }

    @Test func caseIsNotASignal() {
        #expect(RemoteContent.hasRemoteReferences(html: "<IMG SRC=\"HTTPS://X/P.GIF\">"))
    }
}
