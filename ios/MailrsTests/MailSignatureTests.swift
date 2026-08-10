import Testing

@testable import Mailrs

@Suite("Signature")
struct MailSignatureTests {
    @Test("appended under the separator every reader knows")
    func appends() {
        #expect(MailSignature.append(body: "See you then.", signature: "Li Hao\nGOLIA")
            == "See you then.\n\n-- \nLi Hao\nGOLIA")
    }

    /// The trailing space is part of RFC 3676's separator; without it
    /// the line is two hyphens and nothing folds it away.
    @Test("the separator keeps its trailing space")
    func separator() {
        #expect(MailSignature.separator == "-- ")
    }

    @Test("no signature leaves the body exactly as it was")
    func empty() {
        #expect(MailSignature.append(body: "hi", signature: "") == "hi")
        #expect(MailSignature.append(body: "hi", signature: "  \n ") == "hi")
    }

    /// A reply quotes the original beneath what was typed. A second
    /// signature between the two reads as the sender having signed the
    /// other person's message.
    @Test("a body that already signs is left alone")
    func alreadySigned() {
        let signed = "ok\n\n-- \nLi Hao"
        #expect(MailSignature.append(body: signed, signature: "Someone Else") == signed)
        #expect(MailSignature.append(body: "ok\n--\nLi", signature: "X") == "ok\n--\nLi")
        #expect(MailSignature.append(body: "ok\r\n-- \r\nLi", signature: "X")
            == "ok\r\n-- \r\nLi")
    }

    @Test("an empty body is still signed, without leading blank lines")
    func emptyBody() {
        #expect(MailSignature.append(body: "  ", signature: "Li") == "-- \nLi")
    }

    @Test("carriesOne survives CRLF")
    func crlf() {
        #expect(MailSignature.carriesOne("ok\r\n-- \r\nLi"))
    }
}
