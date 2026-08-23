import Testing

@testable import Mailrs

/// Whether to fetch a whole message or only its beginning.
@Suite struct FetchWholeTests {
    /// An ordinary message is fetched whole, which is nearly all of
    /// them.
    @Test func aSmallMessageIsFetchedWhole() {
        #expect(FetchWhole.decide(size: 12_000) == .whole)
        #expect(FetchWhole.decide(size: FetchWhole.threshold) == .whole)
    }

    /// A message with a 25 MB attachment is 25 MB to fetch, and
    /// fetching it to show two lines of text — on somebody's mobile
    /// data, without asking — is noticed on a bill rather than on a
    /// screen.
    @Test func aLargeMessageIsOnlyBegun() {
        #expect(FetchWhole.decide(size: 25_000_000) == .beginning(bytes: FetchWhole.preview))
    }

    /// And all of it once the reader has asked.
    @Test func askingForAllOfItGetsAllOfIt() {
        #expect(FetchWhole.decide(size: 25_000_000, askedForAll: true) == .whole)
    }

    /// **A message of unknown size is fetched whole.** It is usually a
    /// small one, and refusing to show it properly on a guess is worse
    /// than the fetch.
    @Test func anUnknownSizeIsNotTreatedAsLarge() {
        #expect(FetchWhole.decide(size: nil) == .whole)
    }

    /// `<0.262144>` is RFC 3501's partial fetch: offset then length.
    /// The offset is written even though it is zero, because the form
    /// without it means something else — the whole body.
    @Test func thePartialFormCarriesAnOffset() {
        #expect(FetchWhole.bodyItem(.whole) == "BODY.PEEK[]")
        #expect(FetchWhole.bodyItem(.beginning(bytes: 262_144)) == "BODY.PEEK[]<0.262144>")
    }
}
