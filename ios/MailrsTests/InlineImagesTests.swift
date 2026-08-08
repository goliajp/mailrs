import Foundation
import Testing

@testable import Mailrs

struct InlineImagesTests {
    private func part(_ name: String, _ type: String, cid: String?) -> Wire.Attachment {
        Wire.Attachment(filename: name, contentType: type, size: 10, contentId: cid)
    }

    private let body = """
    <p>see</p><img src="cid:logo@x"><img src='cid:Sig@X'><img src=cid:bare@x>
    """

    @Test func findsThePartsTheBodyPointsAt() {
        let atts = [
            part("logo.png", "image/png", cid: "<logo@x>"),
            part("invoice.pdf", "application/pdf", cid: nil),
            part("sig.png", "image/png", cid: "sig@x"),
        ]
        #expect(InlineImages.referenced(html: body, attachments: atts) == [0, 2])
    }

    /// Senders write `<Logo@x>` in the header and `cid:logo@x` in the
    /// body about as often as they agree; both halves are normalised.
    @Test func matchingIgnoresBracketsAndCase() {
        #expect(InlineImages.normalise("  <Logo@X> ") == "logo@x")
        #expect(InlineImages.cids(in: body).contains("sig@x"))
    }

    @Test func anUnquotedAttributeIsStillAReference() {
        #expect(InlineImages.cids(in: body).contains("bare@x"))
    }

    @Test func aBodyWithNoReferencesFetchesNothing() {
        let atts = [part("logo.png", "image/png", cid: "<logo@x>")]
        #expect(InlineImages.referenced(html: "<p>plain</p>", attachments: atts).isEmpty)
        #expect(InlineImages.referenced(html: "", attachments: atts).isEmpty)
    }

    /// A part nobody points at stays a file — this is the difference
    /// between an inline picture and an attachment, and the only thing
    /// that decides it is whether the body asks for it.
    @Test func anUnreferencedPartIsNotInline() {
        let atts = [part("photo.png", "image/png", cid: "<other@x>")]
        #expect(InlineImages.referenced(html: body, attachments: atts).isEmpty)
    }

    @Test func rewritesEveryQuotingStyle() {
        let out = InlineImages.inline(html: body, parts: [
            "logo@x": "data:image/png;base64,AAA",
            "sig@x": "data:image/png;base64,BBB",
            "bare@x": "data:image/png;base64,CCC",
        ])
        #expect(out.contains("src=\"data:image/png;base64,AAA\""))
        #expect(out.contains("src='data:image/png;base64,BBB'"))
        #expect(out.contains("src=data:image/png;base64,CCC"))
        #expect(!out.contains("cid:"))
    }

    /// A part that could not be fetched is left as it was: a broken
    /// image is what the reader would have seen anyway, and a `src`
    /// pointing at nothing is worse.
    @Test func anUnfetchedPartIsLeftAlone() {
        let out = InlineImages.inline(html: body, parts: ["logo@x": "data:image/png;base64,AAA"])
        #expect(out.contains("cid:Sig@X"))
    }

    @Test func theTypeComesFromTheMessage() {
        let uri = InlineImages.dataURI(contentType: "image/gif", data: Data([0x01, 0x02]))
        #expect(uri == "data:image/gif;base64,AQI=")
        #expect(InlineImages.dataURI(contentType: "", data: Data()).hasPrefix("data:application/octet-stream;"))
    }
}

/// The other half: a part the body draws is not also a file to download.
struct InlineAttachmentListTests {
    private func part(_ name: String, _ type: String, cid: String?) -> Wire.Attachment {
        Wire.Attachment(filename: name, contentType: type, size: 10, contentId: cid)
    }

    @Test func inlinePartsLeaveTheFileList() {
        let atts = [
            part("logo.png", "image/png", cid: "<logo@x>"),
            part("invoice.pdf", "application/pdf", cid: nil),
            part("smime.p7s", "application/pkcs7-signature", cid: nil),
        ]
        let files = MessageContent.listable(atts, inlined: [0])
        #expect(files.map(\.attachment.filename) == ["invoice.pdf"])
        // And the surviving file keeps the server's index.
        #expect(files.map(\.index) == [1])
    }
}
