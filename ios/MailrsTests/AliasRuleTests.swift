import Testing

@testable import Mailrs

struct AliasRuleTests {
    @Test func theDomainIsWhatFollowsTheLastAt() {
        #expect(AliasRule.domain(of: "sales@golia.jp") == "golia.jp")
        #expect(AliasRule.domain(of: "Sales <SALES@Golia.JP>") == "golia.jp")
        #expect(AliasRule.domain(of: "nonsense") == "")
    }

    @Test func bothSidesMustBeAddresses() {
        #expect(AliasRule.isCreatable(source: "sales@golia.jp", target: "lihao@golia.jp"))
        #expect(!AliasRule.isCreatable(source: "sales", target: "lihao@golia.jp"))
        #expect(!AliasRule.isCreatable(source: "sales@golia.jp", target: "lihao"))
        #expect(!AliasRule.isCreatable(source: "", target: ""))
    }

    /// An address routed to itself is a loop the server would accept
    /// and the mail would never leave.
    @Test func anAddressCannotAliasToItself() {
        #expect(!AliasRule.isCreatable(source: "sales@golia.jp", target: "sales@golia.jp"))
        #expect(!AliasRule.isCreatable(source: "SALES@Golia.JP", target: "sales@golia.jp"))
    }

    /// Whitespace and display forms are what people paste.
    @Test func pastedFormsAreAccepted() {
        #expect(AliasRule.isCreatable(source: "  sales@golia.jp ",
                                      target: "Li Hao <lihao@golia.jp>"))
    }
}
