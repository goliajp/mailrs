import Foundation
import Testing

@testable import Mailrs

@Suite("Tracking pixels")
struct TrackingPixelsTests {
    @Test("a one-by-one image is removed")
    func removesBeacon() {
        let html = "<p>hi</p><img src=\"https://t.example/o.gif\" width=\"1\" height=\"1\">"
        #expect(TrackingPixels.strip(html: html) == "<p>hi</p>")
    }

    @Test("an inline one-pixel size counts too")
    func inlineStyle() {
        let html = "<img src=\"https://t.example/o\" style=\"width:1px;height:1px\">x"
        #expect(TrackingPixels.strip(html: html) == "x")
    }

    /// The pictures the message is actually about stay.
    @Test("a real image survives")
    func keepsContent() {
        let html = "<img src=\"https://cdn.example/hero.png\" width=\"600\" height=\"200\">"
        #expect(TrackingPixels.strip(html: html) == html)
        let bare = "<img src=\"cid:logo@x\">"
        #expect(TrackingPixels.strip(html: bare) == bare)
    }

    @Test("several beacons in one document all go")
    func severalBeacons() {
        let html = "<img width=1 height=1 src=a><p>body</p><img height='1' width='1' src=b>"
        #expect(TrackingPixels.strip(html: html) == "<p>body</p>")
    }

    @Test("a document with no images is untouched")
    func noImages() {
        let html = "<p>Just text, and a &lt;img&gt; that is not a tag.</p>"
        #expect(TrackingPixels.strip(html: html) == html)
    }

    /// An unterminated tag at the end must not lose the rest of the
    /// document — malformed mail is still mail.
    @Test("an unclosed tag keeps what follows")
    func unclosedTag() {
        let html = "<p>a</p><img src=\"x\" width=\"1\" height=\"1\""
        #expect(TrackingPixels.strip(html: html).hasPrefix("<p>a</p>"))
    }
}
