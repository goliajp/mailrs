import Testing

@testable import Mailrs

/// Where a provider's servers are, and what it calls the secret.
///
/// The table is what makes adding an account one field instead of
/// eight. Its failure modes are quiet: a wrong host sends somebody's
/// password to the wrong machine, and a wrong label for the secret
/// sends them looking for a password they never set.
@Suite struct MailProviderTests {
    @Test func aKnownAddressNeedsNothingElse() {
        let p = MailProvider.forAddress("someone@gmail.com")
        #expect(p?.imapHost == "imap.gmail.com")
        #expect(p?.smtpHost == "smtp.gmail.com")
        #expect(p?.auth == .oauth2)
    }

    /// A suffix match would send somebody's password to Google.
    @Test func aLookalikeDomainIsNotTheProvider() {
        #expect(MailProvider.forDomain("notgmail.com") == nil)
        #expect(MailProvider.forDomain("gmail.com.evil.example") == nil)
    }

    @Test func theCaseOfWhatSomebodyTypedDoesNotMatter() {
        #expect(MailProvider.forAddress("Someone@GMail.COM")?.label == "Gmail")
    }

    /// A plus-address and a name with an @ in it: the domain is what
    /// follows the **last** @.
    @Test func theDomainIsWhatFollowsTheLastAt() {
        #expect(MailProvider.forAddress("first+tag@qq.com")?.label == "QQ")
        #expect(MailProvider.forAddress("odd@name@163.com")?.label == "网易 163")
    }

    /// The provider's own word, because a person will be looking for
    /// exactly that string in its settings.
    @Test func aProviderThatWantsACodeSaysWhatItCallsIt() {
        #expect(MailProvider.forDomain("qq.com")?.secretHelp?.what == "授权码")
        #expect(MailProvider.forDomain("icloud.com")?.secretHelp?.what == "app-specific password")
        // Gmail refuses passwords entirely, so there is no code to make.
        #expect(MailProvider.forDomain("gmail.com")?.secretHelp == nil)
    }

    /// Reading Gmail's All Mail doubles every message in the mailbox.
    @Test func aViewHoldingEverythingIsLeftAlone() {
        let skip = MailProvider.forDomain("gmail.com")?.skipFolders ?? []
        #expect(skip.contains("[Gmail]/All Mail"))
    }

    /// An unknown domain still gets somewhere to start, shown filled
    /// in for correction rather than promised.
    @Test func anUnknownDomainStillGetsAStartingPoint() {
        let g = MailProvider.guess(forDomain: "internal.example.jp")
        #expect(g.imapHost == "imap.internal.example.jp")
        #expect(g.smtpHost == "smtp.internal.example.jp")
        #expect(g.auth == .password)
    }

    /// Aliases people actually type.
    @Test func theOtherNamesForTheSameProviderWork() {
        #expect(MailProvider.forDomain("googlemail.com")?.label == "Gmail")
        #expect(MailProvider.forDomain("hotmail.co.jp")?.label == "Outlook")
        #expect(MailProvider.forDomain("foxmail.com")?.label == "QQ")
        #expect(MailProvider.forDomain("126.com")?.label == "网易 163")
    }

    /// Every provider in the table must be reachable over TLS from the
    /// first byte or with a port that says STARTTLS — a table entry
    /// with a plaintext port is a table entry that leaks a password.
    @Test func noEntryOffersAPlaintextPort() {
        for (domain, p) in MailProvider.table {
            #expect(p.imapPort == 993, "\(domain) imap port \(p.imapPort)")
            #expect(p.smtpPort == 465 || p.smtpPort == 587, "\(domain) smtp port \(p.smtpPort)")
        }
    }
}
