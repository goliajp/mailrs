import Testing

@testable import Mailrs

struct SearchRuleTests {
    @Test func doesNotSearchOnOneCharacter() {
        #expect(SearchRule.query(from: "a") == nil)
        #expect(SearchRule.query(from: "") == nil)
    }

    @Test func searchesOnTwo() {
        #expect(SearchRule.query(from: "ab") == "ab")
    }

    /// Phone keyboards add a space after a word. Without trimming, " a"
    /// is two characters and would fire a search for one.
    @Test func trimsBeforeCounting() {
        #expect(SearchRule.query(from: " a ") == nil)
        #expect(SearchRule.query(from: "  ab  ") == "ab")
        #expect(SearchRule.query(from: "   ") == nil)
    }

    /// Two CJK characters are a real query — often a whole word — and
    /// counting UTF-16 units or bytes instead of characters would let
    /// one through or keep two out.
    @Test func countsCharactersNotBytes() {
        #expect(SearchRule.query(from: "請求") == "請求")
        #expect(SearchRule.query(from: "請") == nil)
    }
}
