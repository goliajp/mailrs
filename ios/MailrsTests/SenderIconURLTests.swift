import Foundation
import Testing

@testable import Mailrs

/// BIMI logos are SVG and some favicons `.ico`; `UIImage` and `NSImage`
/// decode neither, so the app asks the server for a PNG. Without the
/// format those senders show a letter, which looks exactly like a domain
/// with no icon.
struct SenderIconURLTests {
    @Test func asksForAPNG() throws {
        let base = try #require(URL(string: "https://mail.example.test"))
        let url = try MailrsClient.iconURL(base: base, domain: "example.com")
        #expect(url.absoluteString == "https://mail.example.test/api/icon/example.com?format=png")
    }
}
