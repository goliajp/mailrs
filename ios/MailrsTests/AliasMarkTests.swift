import Foundation
import Testing

@testable import Mailrs

struct AliasMarkTests {
    private func alias(_ source: String, _ target: String) -> Wire.Alias {
        Wire.Alias(
            id: Int64(abs(source.hashValue % 100_000)), sourceAddress: source,
            targetAddress: target, domain: "", aliasType: "alias", active: true,
            createdAt: 0
        )
    }

    private let mine = "lihao@golia.jp"
    private var aliases: [Wire.Alias] {
        [
            alias("sales@golia.jp", "lihao@golia.jp"),
            alias("info@golia.ai", "lihao@golia.jp"),
            // Someone else's — it must never be attributed to me.
            alias("support@golia.jp", "roro@golia.jp"),
        ]
    }

    private func via(_ recipients: String) -> String? {
        AliasMark.arrivedVia(recipients: recipients, myAddress: mine, aliases: aliases)
    }

    @Test func mailToTheAddressItselfIsNotViaAnything() {
        #expect(via("lihao@golia.jp") == nil)
        #expect(via("Li Hao <LIHAO@golia.jp>") == nil)
    }

    @Test func mailToAnAliasNamesTheAlias() {
        #expect(via("sales@golia.jp") == "sales@golia.jp")
        #expect(via("Sales <SALES@golia.jp>, bob@example.com") == "sales@golia.jp")
    }

    /// Addressed to both: it is addressed to me. The mark answers "how
    /// did this reach me", and when the direct address is on the line
    /// the answer is "directly".
    @Test func bothAddressesMeansDirect() {
        #expect(via("sales@golia.jp, lihao@golia.jp") == nil)
    }

    /// Another person's alias in the To line says nothing about me.
    @Test func someoneElsesAliasIsNotMine() {
        #expect(via("support@golia.jp") == nil)
        #expect(via("support@golia.jp, bob@example.com") == nil)
    }

    @Test func anUnrelatedRecipientListMarksNothing() {
        #expect(via("bob@example.com, carol@example.net") == nil)
        #expect(via("") == nil)
    }

    /// A catch-all names the address the sender used, not the pattern
    /// that routed it: "via @golia.jp" is a rule, and the reader wants
    /// to know which address of theirs is in circulation.
    @Test func aCatchAllNamesTheAddressThatWasWrittenTo() {
        let withCatchAll = aliases + [alias("@golia.jp", "lihao@golia.jp")]
        let mark = AliasMark.arrivedVia(
            recipients: "newsletter@golia.jp", myAddress: mine, aliases: withCatchAll
        )
        #expect(mark == "newsletter@golia.jp")
    }

    /// And an exact alias wins over the catch-all that would also match.
    @Test func anExactAliasOutranksTheCatchAll() {
        let withCatchAll = aliases + [alias("*@golia.jp", "lihao@golia.jp")]
        let mark = AliasMark.arrivedVia(
            recipients: "sales@golia.jp", myAddress: mine, aliases: withCatchAll
        )
        #expect(mark == "sales@golia.jp")
    }

    /// A catch-all on one domain says nothing about another.
    @Test func aCatchAllIsBoundToItsDomain() {
        #expect(AliasMark.isCatchAll("@golia.jp", for: "golia.jp"))
        #expect(AliasMark.isCatchAll("*@golia.jp", for: "GOLIA.JP"))
        #expect(!AliasMark.isCatchAll("@golia.jp", for: "golia.ai"))
        #expect(!AliasMark.isCatchAll("sales@golia.jp", for: "golia.jp"))
    }

    /// Without a signed-in address there is nothing to be "via".
    @Test func noIdentityMarksNothing() {
        #expect(AliasMark.arrivedVia(recipients: "sales@golia.jp", myAddress: "", aliases: aliases) == nil)
    }
}
