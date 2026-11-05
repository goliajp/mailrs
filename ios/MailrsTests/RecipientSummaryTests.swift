import Testing

@testable import Mailrs

/// A promotional mail listed several hundred addresses in `To:`. The
/// header shows such a list as the reader, three names and a count.
@Suite struct RecipientSummaryTests {
    private func contacts(_ n: Int, from: Int = 0) -> [String] {
        (from..<(from + n)).map { "contact\($0) <c\($0)@promo.example>" }
    }

    @Test func aLongListCollapsesWithTheReaderFirst() throws {
        let to = (contacts(150) + ["Lihao <LiHao@golia.jp>"] + contacts(149, from: 150))
            .joined(separator: ", ")
        let summary = try #require(RecipientSummary.of(to, myAddress: "lihao@golia.jp"))
        #expect(summary.me)
        #expect(summary.named == ["contact0", "contact1", "contact2"])
        #expect(summary.rest == 296)
        #expect(summary.total == 300)
    }

    /// Bcc'd: the reader is not on the list, so the line does not say
    /// "me".
    @Test func aReaderWhoWasOnlyBccIsNotClaimed() throws {
        let to = contacts(300).joined(separator: ", ")
        let summary = try #require(RecipientSummary.of(to, myAddress: "lihao@golia.jp"))
        #expect(!summary.me)
        #expect(summary.named == ["contact0", "contact1", "contact2"])
        #expect(summary.rest == 297)
    }

    /// A quoted comma is one person, not two, in the count as anywhere.
    @Test func theCountUsesTheSameSplitAsReplyAll() throws {
        let to = ([#""Lovelace, Ada" <ada@example.com>"#] + contacts(8)).joined(separator: ", ")
        let summary = try #require(RecipientSummary.of(to, myAddress: ""))
        #expect(summary.total == 9)
        #expect(summary.named.first == "Lovelace, Ada")
    }

    @Test func aShortListIsShownWhole() {
        #expect(RecipientSummary.of(contacts(8).joined(separator: ", "), myAddress: "c0@promo.example") == nil)
        #expect(RecipientSummary.of("", myAddress: "lihao@golia.jp") == nil)
    }
}
