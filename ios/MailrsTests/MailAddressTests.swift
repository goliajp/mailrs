import Testing

@testable import Mailrs

struct MailAddressTests {
    @Test func anAddressHasANameOneAtSignAndADottedDomain() {
        for ok in ["qa@golia.jp", "a.b+tag@mail.example.co.jp"] {
            #expect(MailAddress.looksLike(ok), "\(ok)")
        }
        for bad in ["", "qa", "qa@", "@golia.jp", "qa@golia", "qa@golia.", "qa@.jp", "a@b@c.jp", "qa @golia.jp", "去啊@我俩."] {
            #expect(!MailAddress.looksLike(bad), "\(bad)")
        }
    }
}
