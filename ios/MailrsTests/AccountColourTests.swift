import Testing

@testable import Mailrs

/// The dot that says which account a row came from.
@Suite struct AccountColourTests {
    /// The same account keeps its colour — the reason this is a fold
    /// over the id and not `hashValue`, which Swift seeds per process
    /// and which would therefore repaint every list on every launch.
    @Test func theSameAccountAlwaysGetsTheSameColour() {
        let first = AccountColour.forId("abc-123")
        for _ in 0..<20 { #expect(AccountColour.forId("abc-123") == first) }
    }

    /// Always a colour that exists, whatever the id looks like.
    @Test func theColourIsAlwaysFromThePalette() {
        for id in ["", "a", "one@example.com", String(repeating: "z", count: 300), "空"] {
            #expect(AccountColour.palette.contains(AccountColour.forId(id)))
        }
    }

    /// Not a guarantee that two given accounts differ — eight hues and
    /// more than eight accounts must collide — but the spread has to be
    /// real, or the dot says nothing.
    @Test func differentAccountsSpreadAcrossThePalette() {
        let ids = (0..<80).map { "account-\($0)" }
        #expect(Set(ids.map { AccountColour.forId($0) }).count == AccountColour.palette.count)
    }
}
