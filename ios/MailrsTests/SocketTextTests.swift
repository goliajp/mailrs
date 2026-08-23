import Foundation
import Testing

@testable import Mailrs

/// The byte-preserving reader, and getting back out of it.
///
/// This is what stands between a Shift_JIS message and a screen of
/// replacement characters: the socket may not decide what a message
/// says before the message has said what it is.
@Suite struct SocketTextTests {
    /// Every byte value survives the trip, including the ones no text
    /// encoding would accept.
    @Test func everyByteSurvivesTheRoundTrip() {
        let all = Data((0...255).map { UInt8($0) })
        #expect(SocketText.bytes(SocketText.latin1(all)) == all)
    }

    /// A UTF-8 body read as latin-1 is recovered exactly.
    @Test func utf8ContentIsRecovered() {
        let original = "café — 日本語"
        #expect(SocketText.utf8(SocketText.latin1(Data(original.utf8))) == original)
    }

    /// Bytes that are not UTF-8 keep what they were rather than
    /// becoming replacement characters — a latin-1 folder name should
    /// read as latin-1.
    @Test func nonUtf8BytesAreLeftAsTheyWere() {
        let asRead = SocketText.latin1(Data([0x63, 0x61, 0x66, 0xE9]))
        #expect(SocketText.utf8(asRead) == "café")
    }

    /// ASCII, which is nearly all of a mail session, is untouched.
    @Test func asciiIsUnchangedEitherWay() {
        let line = "a1 OK [READ-WRITE] SELECT completed"
        #expect(SocketText.utf8(line) == line)
        #expect(SocketText.bytes(line) == Data(line.utf8))
    }
}
