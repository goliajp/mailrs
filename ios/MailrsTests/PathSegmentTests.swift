import Foundation
import Testing

@testable import Mailrs

@Suite("Path segments")
struct PathSegmentTests {
    /// The one that was reported: GitHub's thread ids are four slashes
    /// deep, so interpolated raw they became four path segments and the
    /// request reached some other route entirely.
    @Test("a thread id with slashes becomes one segment")
    func slashesAreEncoded() {
        let id = "goliajp/kevy/check-suites/CS_kwDOSqTW2M8AAAATyPliHg/1786297239@github.com"
        let encoded = MailrsClient.segment(id)
        #expect(!encoded.contains("/"))
        #expect(encoded.contains("%2F"))
        #expect(encoded.removingPercentEncoding == id)
    }

    @Test("the ordinary ids survive unchanged")
    func plainIdsAreLeftAlone() {
        #expect(MailrsClient.segment("a48529b44b1b190f") == "a48529b44b1b190f")
        #expect(MailrsClient.segment("m1.eml") == "m1.eml")
        #expect(MailrsClient.segment("a-b_c~d") == "a-b_c~d")
    }

    /// `@`, `+` and `=` all appear in real Message-IDs and all mean
    /// something else in a URL.
    @Test("the characters a Message-ID actually carries")
    func messageIdCharacters() {
        let encoded = MailrsClient.segment("a+b=c@d.example")
        #expect(encoded == "a%2Bb%3Dc%40d.example")
        #expect(encoded.removingPercentEncoding == "a+b=c@d.example")
    }

    /// Building the URL by hand rather than with
    /// `appendingPathComponent`, which escapes the `%` of an already
    /// encoded segment and breaks it a second way.
    @Test("the encoded segment survives into the URL")
    func urlKeepsTheEncoding() {
        let client = MailrsClient(baseURL: URL(string: "https://mail.example")!, token: "t")
        let url = client.url("/api/conversations/\(MailrsClient.segment("a/b@c"))")
        #expect(url.absoluteString == "https://mail.example/api/conversations/a%2Fb%40c")
    }
}
