import Foundation
import Testing

@testable import Mailrs

/// How large a message this client can send.
@Suite struct OutgoingLimitsTests {
    private func draft(_ sizes: [Int]) -> OutgoingMessage.Draft {
        var d = OutgoingMessage.Draft(from: "me@example.com", to: ["you@example.com"])
        d.body = "words"
        d.attachments = sizes.enumerated().map { index, size in
            .init(
                filename: "f\(index).bin", mimeType: "application/octet-stream",
                bytes: Data(count: size))
        }
        return d
    }

    /// No attachment is never too large.
    @Test func aPlainMessageAlwaysPasses() {
        #expect(OutgoingLimits.check(draft([])) == .ok)
    }

    /// An ordinary photo passes, which is nearly every message.
    @Test func anOrdinaryAttachmentPasses() {
        #expect(OutgoingLimits.check(draft([3_000_000])) == .ok)
    }

    /// **The limit is on the encoded message, not on the files.**
    /// base64 makes it a third larger, so 20 MB of photos is 27 MB on
    /// the wire — and a person refused at that point reads it as the
    /// client losing their message.
    @Test func theEncodingCountsAgainstTheLimit() {
        // Under the raw limit, over the encoded one.
        #expect(OutgoingLimits.check(draft([20_000_000])) != .ok)
    }

    /// Several files add up, because the server adds them up.
    @Test func attachmentsAreCountedTogether() {
        #expect(OutgoingLimits.check(draft([10_000_000, 10_000_000])) != .ok)
        #expect(OutgoingLimits.check(draft([2_000_000, 2_000_000])) == .ok)
    }

    /// Reported in the units the person chose the files in — they
    /// attached 26 MB of photos, and telling them the message is 35 MB
    /// is telling them about arithmetic they did not do.
    @Test func theMessageNamesWhatWasAttached() {
        guard case let .tooLarge(attached, limit) = OutgoingLimits.check(draft([26_000_000]))
        else {
            Issue.record("26 MB was accepted")
            return
        }
        #expect(attached == 26_000_000)
        #expect(limit < OutgoingLimits.encodedMax)
    }

    /// A server that states its own `SIZE` has told the truth where
    /// the default has only guessed.
    @Test func aServerLimitCanReplaceTheGuess() {
        #expect(OutgoingLimits.check(draft([3_000_000]), limit: 1_000_000) != .ok)
    }
}
