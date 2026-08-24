import Testing

@testable import Mailrs

/// Dot-stuffing a message that arrives in pieces.
@Suite struct DotStufferTests {
    private func whole(_ text: String) -> String { DotStuffer().feed(text) }

    private func inPieces(_ text: String, size: Int) -> String {
        let stuffer = DotStuffer()
        var out = ""
        var rest = Substring(text)
        while !rest.isEmpty {
            let take = rest.prefix(size)
            out += stuffer.feed(String(take))
            rest = rest.dropFirst(take.count)
        }
        return out
    }

    /// The ordinary case, and the reason the rule exists.
    @Test func aLineBeginningWithADotGetsAnother() {
        #expect(whole("a\r\n.\r\nb") == "a\r\n..\r\nb")
        #expect(whole(".hidden") == "..hidden")
    }

    /// A dot that is not at a line start is just a dot.
    @Test func aDotInsideALineIsLeftAlone() {
        #expect(whole("see www.example.com") == "see www.example.com")
        #expect(whole("end.\r\n") == "end.\r\n")
    }

    /// **The whole point.** A chunk can end exactly on the line break
    /// and the next begin with the dot — a stuffer that forgets treats
    /// it as mid-line text, and the message is truncated there while
    /// arriving as a complete-looking message that stops halfway.
    @Test func aDotAtTheStartOfTheNextChunkIsStillAtALineStart() {
        let stuffer = DotStuffer()
        let first = stuffer.feed("hello\r\n")
        let second = stuffer.feed(".\r\nworld")
        #expect(first + second == "hello\r\n..\r\nworld")
    }

    /// And at **every** split, not just the interesting one. A message
    /// cut at each position in turn must come out the same as one cut
    /// nowhere.
    @Test func splittingAnywhereGivesTheSameAnswerAsNotSplitting() {
        let message = "Subject: x\r\n\r\n.\r\n..\r\nnormal\r\n.dotted\r\nwww.example.com\r\n."
        let reference = whole(message)
        for size in 1...message.count {
            #expect(
                inPieces(message, size: size) == reference,
                Comment(rawValue: "split every \(size) characters"))
        }
    }

    /// A lone CR does not start a line. In a message that is a stray
    /// byte, and treating it as a break would stuff a dot that is not
    /// at a line start.
    @Test func aLoneCarriageReturnDoesNotStartALine() {
        #expect(whole("a\r.b") == "a\r.b")
    }

    /// An empty piece changes nothing, including the state.
    @Test func anEmptyChunkIsHarmless() {
        let stuffer = DotStuffer()
        #expect(stuffer.feed("x\r\n") == "x\r\n")
        #expect(stuffer.feed("") == "")
        #expect(stuffer.feed(".y") == "..y")
    }
}
