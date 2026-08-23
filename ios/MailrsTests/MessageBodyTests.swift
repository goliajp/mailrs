import Foundation
import Testing

@testable import Mailrs

/// Pulling the readable part out of a raw message.
@Suite struct MessageBodyTests {
    private func raw(_ s: String) -> Data { Data(s.utf8) }

    @Test func aPlainMessageIsItsOwnBody() {
        let out = MessageBody.extract(raw("Subject: hi\r\n\r\nHello there.\r\n"))
        #expect(out.text == "Hello there.\r\n")
        #expect(out.isHTML == false)
    }

    /// No `Content-Type` at all is `text/plain; charset=us-ascii` by
    /// RFC 2045 — and far more common than any declared type.
    @Test func aMessageWithNoContentTypeIsStillText() {
        #expect(MessageBody.extract(raw("From: a\n\nbody")).text == "body")
    }

    @Test func quotedPrintableIsDecoded() {
        let message = """
            Content-Type: text/plain; charset=utf-8\r
            Content-Transfer-Encoding: quoted-printable\r
            \r
            caf=C3=A9 and a very long line that was wrapped right =\r
            here\r
            """
        // The trailing CR is the body's own: a soft break eats the
        // `=\r\n` that wrapped the line, and what is left is what was
        // written, ending where the message ends.
        #expect(
            MessageBody.extract(raw(message)).text
                == "café and a very long line that was wrapped right here\r")
    }

    @Test func base64IsDecodedAcrossLines() {
        let body = Data("Hello, wrapped base64 body.".utf8).base64EncodedString()
        let wrapped = body.prefix(8) + "\r\n" + body.dropFirst(8)
        let message = "Content-Transfer-Encoding: base64\r\n\r\n\(wrapped)"
        #expect(MessageBody.extract(raw(message)).text == "Hello, wrapped base64 body.")
    }

    /// The charset is inside the message, which is why this works on
    /// bytes: decoding as UTF-8 on the way in would already have lost
    /// these.
    @Test func aDeclaredCharsetIsHonoured() {
        var message = Data("Content-Type: text/plain; charset=iso-8859-1\r\n\r\n".utf8)
        message.append(contentsOf: [0x63, 0x61, 0x66, 0xE9])  // café in latin-1
        #expect(MessageBody.extract(message).text == "café")
    }

    /// The same message twice: show the one a person can read.
    @Test func alternativePrefersPlainText() {
        let message = """
            Content-Type: multipart/alternative; boundary="x"\r
            \r
            preamble nobody sees\r
            --x\r
            Content-Type: text/plain\r
            \r
            the plain one\r
            --x\r
            Content-Type: text/html\r
            \r
            <p>the markup one</p>\r
            --x--\r
            epilogue nobody sees\r
            """
        let out = MessageBody.extract(raw(message))
        #expect(out.text.contains("the plain one"))
        #expect(out.isHTML == false)
        #expect(!out.text.contains("preamble"))
        #expect(!out.text.contains("epilogue"))
    }

    /// Markup when that is all there is — flagged, so the caller
    /// renders it rather than showing somebody their own angle
    /// brackets.
    @Test func alternativeFallsBackToHTML() {
        let message = """
            Content-Type: multipart/alternative; boundary="x"\r
            \r
            --x\r
            Content-Type: text/html\r
            \r
            <p>only markup</p>\r
            --x--\r
            """
        let out = MessageBody.extract(raw(message))
        #expect(out.isHTML)
        #expect(out.text.contains("only markup"))
    }

    /// A message with an attachment: the message is the message.
    @Test func mixedSkipsTheAttachment() {
        let message = """
            Content-Type: multipart/mixed; boundary="b"\r
            \r
            --b\r
            Content-Type: text/plain\r
            \r
            see attached\r
            --b\r
            Content-Type: application/pdf\r
            Content-Transfer-Encoding: base64\r
            \r
            JVBERi0xLjQK\r
            --b--\r
            """
        #expect(MessageBody.extract(raw(message)).text.contains("see attached"))
    }

    /// A `mixed` whose first piece is an `alternative`, which is what
    /// most mail with an attachment actually looks like.
    @Test func aNestedAlternativeIsReadThrough() {
        let message = """
            Content-Type: multipart/mixed; boundary="outer"\r
            \r
            --outer\r
            Content-Type: multipart/alternative; boundary="inner"\r
            \r
            --inner\r
            Content-Type: text/plain\r
            \r
            nested plain\r
            --inner\r
            Content-Type: text/html\r
            \r
            <p>nested markup</p>\r
            --inner--\r
            --outer--\r
            """
        let out = MessageBody.extract(raw(message))
        #expect(out.text.contains("nested plain"))
        #expect(out.isHTML == false)
    }

    /// A boundary with a semicolon in it, quoted. Splitting the
    /// parameter list on every semicolon loses the rest of the name and
    /// then nothing matches.
    @Test func aQuotedBoundaryMaySpanASemicolon() {
        let message = """
            Content-Type: multipart/alternative; boundary="a;b"\r
            \r
            --a;b\r
            Content-Type: text/plain\r
            \r
            found it\r
            --a;b--\r
            """
        #expect(MessageBody.extract(raw(message)).text.contains("found it"))
    }

    /// Nothing to show is not a crash.
    @Test func brokenInputIsEmptyRatherThanFatal() {
        #expect(MessageBody.extract(Data()) == MessageBody.Display.empty)
        #expect(MessageBody.extract(raw("Subject: only headers\r\n")).text.isEmpty)
        let noBoundary = "Content-Type: multipart/alternative\r\n\r\nsomething"
        #expect(MessageBody.extract(raw(noBoundary)).text.contains("something"))
    }

    /// An attachment on its own is not text, and showing its bytes as
    /// text is a screen of noise.
    @Test func aNonTextPartShowsNothing() {
        let message = "Content-Type: image/png\r\n\r\n\u{89}PNG"
        #expect(MessageBody.extract(raw(message)) == MessageBody.Display.empty)
    }
}
