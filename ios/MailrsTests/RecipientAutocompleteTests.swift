import Testing

@testable import Mailrs

struct RecipientAutocompleteTests {
    @Test func theTokenIsWhatFollowsTheLastSeparator() {
        #expect(RecipientAutocomplete.currentToken(of: "ali") == "ali")
        #expect(RecipientAutocomplete.currentToken(of: "a@b.com, ke") == "ke")
        #expect(RecipientAutocomplete.currentToken(of: "a@b.com; ke") == "ke")
        #expect(RecipientAutocomplete.currentToken(of: "a@b.com,") == "")
        #expect(RecipientAutocomplete.currentToken(of: "  ") == "")
    }

    /// Two characters to ask, and a complete address asks nothing.
    @Test func suggestionsWaitForTwoCharactersAndStopAtAnAddress() {
        #expect(!RecipientAutocomplete.shouldSuggest(for: "a"))
        #expect(RecipientAutocomplete.shouldSuggest(for: "al"))
        #expect(!RecipientAutocomplete.shouldSuggest(for: "alice@example.com"))
    }

    /// The picked contact lands as its addr-spec — display forms are
    /// for screens, the wire takes bare addresses.
    @Test func completionReplacesTheTokenWithTheAddrSpec() {
        #expect(RecipientAutocomplete.completing("ali", with: "Alice Smith <alice@example.com>")
            == "alice@example.com, ")
        #expect(RecipientAutocomplete.completing("bob@x.com, ke", with: "Keiri <keiri@example.co.jp>")
            == "bob@x.com, keiri@example.co.jp, ")
    }
}
