import Foundation
import Testing

@testable import Mailrs

/// The cases are the real ones. Six of these From headers were sitting
/// in the mailbox when the rule was written; the two that must stay
/// silent were sitting there too.
struct SenderClaimTests {
    private func contradiction(_ name: String, _ address: String) -> String? {
        SenderClaim.contradictedDomain(displayName: name, address: address)
    }

    @Test func brandImpersonationIsNamed() {
        #expect(contradiction("Amazon.co.jp", "<no-reply@mail07.jqjintaiyang.com>")
                == "mail07.jqjintaiyang.com")
        #expect(contradiction("【zon.co.jp】ご注文", "x@drink.thinking-progress.com")
                == "drink.thinking-progress.com")
    }

    /// The one that matters most here: mail claiming to be from this
    /// deployment's own domain, sent from somewhere else entirely.
    @Test func aClaimOnOurOwnDomainIsNamed() {
        #expect(contradiction("golia.jp Support", "billing@exportesram-ems.cam")
                == "exportesram-ems.cam")
    }

    /// A subdomain of the claimed domain is the ordinary case — every
    /// large sender mails from `email.` or `mail.` — and must stay
    /// silent, or the mark fires on almost everything.
    @Test func aSubdomainOfTheClaimIsSilent() {
        #expect(contradiction("Amazon.co.jp", "no-reply@email.amazon.co.jp") == nil)
        #expect(contradiction("GitHub.com", "notifications@github.com") == nil)
        #expect(contradiction("mail.golia.jp", "x@golia.jp") == nil)
    }

    /// Most display names claim nothing: 206 of 1,500 contained a
    /// domain-like token at all, so silence is the common answer.
    @Test func aNameThatClaimsNothingIsSilent() {
        #expect(contradiction("Alice Smith", "alice@example.com") == nil)
        #expect(contradiction("", "alice@example.com") == nil)
        #expect(contradiction("経理部", "keiri@example.co.jp") == nil)
    }

    /// A bare suffix is not a claim — "co.jp" on its own says nothing
    /// about who sent it, and treating it as a domain fired on twenty
    /// ordinary messages in the first measurement.
    @Test func aBareSuffixIsNotAClaim() {
        #expect(SenderClaim.claimedDomain(in: "co.jp") == nil)
        #expect(SenderClaim.claimedDomain(in: "Sales co.jp") == nil)
        #expect(SenderClaim.claimedDomain(in: "yahoo.co.jp") == "yahoo.co.jp")
    }

    @Test func punctuationAroundTheNameDoesNotHideIt() {
        #expect(SenderClaim.claimedDomain(in: "【Amazon.co.jp】") == "amazon.co.jp")
        #expect(SenderClaim.claimedDomain(in: "\"PayPal.com\" Service") == "paypal.com")
    }

    @Test func anAddressWithoutADomainIsNotJudged() {
        #expect(contradiction("Amazon.com", "postmaster") == nil)
        #expect(SenderClaim.domain(of: "Alice <alice@example.com>") == "example.com")
    }
}
