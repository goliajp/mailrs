import Foundation
import Testing

@testable import Mailrs

/// The part of opening a message that needs no server: raw bytes in,
/// something a person can read out.
@Suite struct MessageReaderTests {
    private func raw(_ s: String) -> Data { Data(s.utf8) }

    @Test func plainTextArrivesAsItself() {
        let out = MessageReader.display(of: raw("Subject: x\r\n\r\nJust words.\r\n"))
        #expect(out.text == "Just words.\r\n")
        #expect(out.fromHTML == false)
    }

    /// Markup becomes text rather than being rendered: that is what
    /// stops a message asking somebody else's server for an image and
    /// reporting that it was read.
    @Test func markupBecomesText() {
        let message = """
            Content-Type: text/html; charset=utf-8\r
            \r
            <html><head><style>p{color:red}</style></head>\r
            <body><p>Hello <b>there</b>.</p><p>Second line.</p>\r
            <img src="https://tracker.example/pixel.gif?id=42">\r
            </body></html>\r
            """
        let out = MessageReader.display(of: raw(message))
        #expect(out.fromHTML)
        #expect(out.text == "Hello there.\nSecond line.")
        // The whole point, asserted rather than assumed.
        #expect(!out.text.contains("tracker.example"))
        #expect(!out.text.contains("color:red"))
    }

    /// The plain half of a two-part message wins, and nothing says it
    /// came from markup, because it did not.
    @Test func alternativeReadsAsPlain() {
        let message = """
            Content-Type: multipart/alternative; boundary="b"\r
            \r
            --b\r
            Content-Type: text/plain\r
            \r
            the readable one\r
            --b\r
            Content-Type: text/html\r
            \r
            <p>the other one</p>\r
            --b--\r
            """
        let out = MessageReader.display(of: raw(message))
        #expect(out.text.contains("the readable one"))
        #expect(out.fromHTML == false)
    }

    /// A message with nothing showable is empty text, not a crash and
    /// not a failure — the screen says so in its own words.
    @Test func anAttachmentOnlyMessageHasNothingToShow() {
        let message = "Content-Type: application/pdf\r\n\r\nJVBERi0="
        #expect(MessageReader.display(of: raw(message)).text.isEmpty)
    }
}
