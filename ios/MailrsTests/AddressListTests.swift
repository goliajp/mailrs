import Testing

@testable import Mailrs

/// Mirrors `web/src/lib/__tests__` around `parseAddressList`, so a
/// divergence between the two clients shows up as a diff between two
/// files meant to match.
struct AddressListTests {
    @Test func splitsOnCommaAndSemicolon() {
        #expect(AddressList.parse("a@b.jp, c@d.jp") == ["a@b.jp", "c@d.jp"])
        #expect(AddressList.parse("a@b.jp; c@d.jp") == ["a@b.jp", "c@d.jp"])
        #expect(AddressList.parse("a@b.jp,c@d.jp; e@f.jp") == ["a@b.jp", "c@d.jp", "e@f.jp"])
    }

    @Test func trimsAndDropsEmpties() {
        #expect(AddressList.parse("  a@b.jp  ,, ; c@d.jp ") == ["a@b.jp", "c@d.jp"])
        #expect(AddressList.parse("") == [])
        #expect(AddressList.parse(" , ; ") == [])
    }

    @Test func needsAtLeastOnePlausibleAddress() {
        #expect(!AddressList.isSendable(""))
        #expect(!AddressList.isSendable("not-an-address"))
        #expect(!AddressList.isSendable("@nolocal.jp"))
        #expect(!AddressList.isSendable("a@nodot"))
        #expect(!AddressList.isSendable("a@trailing."))
        #expect(AddressList.isSendable("a@b.jp"))
    }

    /// Every entry has to be plausible, not just the first — a trailing
    /// typo after a valid address is the common case and the one a "first
    /// entry wins" check would send anyway.
    @Test func rejectsWhenAnyEntryIsNotAnAddress() {
        #expect(!AddressList.isSendable("a@b.jp, oops"))
        #expect(AddressList.isSendable("a@b.jp, c@d.co.jp"))
    }
}
