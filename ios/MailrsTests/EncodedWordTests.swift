import Foundation
import Testing

@testable import Mailrs

/// RFC 2047 encoded words.
///
/// Without this every Japanese or Chinese subject in the list is a run
/// of `=?UTF-8?B?` gibberish — the most visible way a mail client can
/// look broken.
@Suite struct EncodedWordTests {
    @Test func aBase64WordBecomesText() {
        // "会議の件" in UTF-8 base64.
        #expect(EncodedWord.decode("=?UTF-8?B?5Lya6K2w44Gu5Lu2?=") == "会議の件")
    }

    @Test func aQuotedPrintableWordBecomesText() {
        #expect(EncodedWord.decode("=?UTF-8?Q?caf=C3=A9?=") == "café")
        // `_` is a space in Q-encoding, not an underscore.
        #expect(EncodedWord.decode("=?UTF-8?Q?two_words?=") == "two words")
    }

    /// A subject is often half encoded and half not, and re-encoding
    /// the plain half would corrupt it.
    @Test func plainTextAroundAWordIsUntouched() {
        #expect(EncodedWord.decode("Re: =?UTF-8?B?5Lya6K2w?= (2)") == "Re: 会議 (2)")
    }

    /// **RFC 2047 §6.2.** Whitespace *between two encoded words* is
    /// there so they can be folded — it is not part of the text. A
    /// decoder that keeps it puts a space in the middle of every long
    /// CJK subject.
    @Test func theGapBetweenTwoWordsIsNotText() {
        let s = "=?UTF-8?B?5Lya6K2w?= =?UTF-8?B?44Gu5Lu2?="
        #expect(EncodedWord.decode(s) == "会議の件")
    }

    /// But a gap between a word and plain text **is** text.
    @Test func theGapBeforePlainTextSurvives() {
        #expect(EncodedWord.decode("=?UTF-8?B?5Lya6K2w?= today") == "会議 today")
    }

    /// Base64 without padding is common in the wild and decodes to
    /// nothing without it.
    @Test func anUnpaddedWordStillDecodes() {
        #expect(EncodedWord.decode("=?UTF-8?B?5Lya6K2w44Gu5Lu2?=") == "会議の件")
        #expect(EncodedWord.decode("=?UTF-8?B?YWJj?=") == "abc")
    }

    /// Mojibake somebody can report beats text this app invented, so
    /// an unknown charset leaves the raw word visible.
    @Test func anUnknownCharsetIsLeftAlone() {
        let s = "=?X-MADE-UP?B?YWJj?="
        #expect(EncodedWord.decode(s) == s)
    }

    @Test func somethingThatIsNotAWordIsLeftAlone() {
        #expect(EncodedWord.decode("plain subject") == "plain subject")
        #expect(EncodedWord.decode("=? not a word") == "=? not a word")
        #expect(EncodedWord.decode("") == "")
    }
}
