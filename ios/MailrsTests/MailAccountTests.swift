import Foundation
import Testing

@testable import Mailrs

/// A mailbox somewhere else, as this app holds it.
@Suite struct MailAccountTests {
    /// One address, one row — adding the same account twice must be
    /// the same row rather than two, and a keychain item has to
    /// survive a list rebuilt from scratch.
    @Test func theSameAddressIsAlwaysTheSameId() {
        #expect(MailAccount.id(for: "me@gmail.com") == MailAccount.id(for: "me@gmail.com"))
        #expect(MailAccount.id(for: "Me@Gmail.com") == MailAccount.id(for: "me@gmail.com"))
        #expect(MailAccount.id(for: "me@gmail.com") != MailAccount.id(for: "you@gmail.com"))
    }

    /// A known address needs nothing else typed.
    @Test func aKnownProviderFillsItselfIn() {
        let a = MailAccount.make(address: "someone@qq.com")
        #expect(a.imapHost == "imap.qq.com")
        #expect(a.smtpPort == 465)
        #expect(a.auth == .appPassword)
        #expect(a.provider == "qq")
    }

    /// And an unknown one still gets somewhere to start.
    @Test func anUnknownDomainIsMarkedCustom() {
        let a = MailAccount.make(address: "me@internal.example.jp")
        #expect(a.provider == "custom")
        #expect(a.imapHost == "imap.internal.example.jp")
    }

    /// The row is the thing that gets logged, encoded and shown. It
    /// must not be the thing that carries a password.
    ///
    /// Checked against the secret itself rather than against the word
    /// "password": `auth: "app_password"` names the **kind** of
    /// credential the server wants, which the row is right to hold.
    @Test func theRowHoldsNoSecret() throws {
        let a = MailAccount.make(address: "me@qq.com")
        AccountStore.saveSecret("hunter2-not-a-real-one", for: a.id)
        defer { AccountStore.deleteSecret(for: a.id) }

        let json = String(decoding: try JSONEncoder().encode(a), as: UTF8.self)
        #expect(!json.contains("hunter2-not-a-real-one"), "the row carries the secret")
        // And the fields it does have are the ones a person could see
        // on a settings screen without harm.
        let mirror = Mirror(reflecting: a)
        let names = Set(mirror.children.compactMap(\.label))
        #expect(names.isDisjoint(with: ["password", "secret", "token", "credential"]))
    }

    /// Said before spending thirty seconds finding out that a blank
    /// host does not resolve.
    @Test func whatIsMissingIsSaidInWordsSomebodyCanActOn() {
        var a = MailAccount.make(address: "me@example.jp")
        #expect(a.problem == nil)
        a.imapHost = ""
        #expect(a.problem == "The incoming server needs a name")
        a = MailAccount.make(address: "not-an-address")
        #expect(a.problem == "That is not an email address")
    }

    @Test func aRowWithNoNameShowsItsAddress() {
        var a = MailAccount.make(address: "me@qq.com")
        #expect(a.title == "me@qq.com")
        a.displayName = "Work"
        #expect(a.title == "Work")
    }

    /// The server is told the login name when there is one, and the
    /// address when there is not.
    @Test func theLoginNameFallsBackToTheAddress() {
        var a = MailAccount.make(address: "me@example.jp")
        #expect(a.loginName == "me@example.jp")
        a.login = "me"
        #expect(a.loginName == "me")
    }
}

/// The words somebody reads when connecting fails.
@Suite struct AccountConnectionTests {
    /// The bracket is for programs. `[AUTHENTICATIONFAILED] Invalid
    /// credentials` reads better as `Invalid credentials`.
    @Test func theResponseCodeIsNotShownToAPerson() {
        #expect(AccountConnection.readable("[AUTHENTICATIONFAILED] Invalid credentials")
            == "Invalid credentials")
        #expect(AccountConnection.readable("[ALERT] Please use an app password")
            == "Please use an app password")
    }

    /// SMTP puts an enhanced status code in front for the same reason.
    @Test func theEnhancedStatusCodeIsNotShownEither() {
        #expect(AccountConnection.readable("5.7.8 Username and Password not accepted")
            == "Username and Password not accepted")
    }

    /// A reason with nothing to strip survives whole, and an empty one
    /// falls back rather than showing a blank line.
    @Test func aPlainReasonIsLeftAlone() {
        #expect(AccountConnection.readable("Try again later") == "Try again later")
        #expect(AccountConnection.readable("[ONLYACODE]") == "[ONLYACODE]")
    }

    /// A version number is not a status code — stripping it would eat
    /// the first word of the reason.
    @Test func somethingThatIsNotAStatusCodeIsNotStripped() {
        #expect(AccountConnection.readable("1.2.3.4 is not permitted") == "1.2.3.4 is not permitted")
    }
}

/// Server settings somebody types in themselves.
@MainActor
@Suite struct ManualEndpointTests {
    private func model(_ ih: String, _ ip: String, _ sh: String, _ sp: String)
        -> MailAccountsModel
    {
        let m = MailAccountsModel()
        m.draft.imapHost = ih
        m.draft.imapPort = ip
        m.draft.smtpHost = sh
        m.draft.smtpPort = sp
        return m
    }

    @Test func bothServersGoOutWhenBothAreComplete() {
        let out = model("imap.x.jp", "993", "smtp.x.jp", "465").manualEndpoints()
        #expect(out?.imapHost == "imap.x.jp")
        #expect(out?.imapPort == 993)
        #expect(out?.smtpPort == 465)
    }

    /// A half-filled pair is refused here rather than by the server
    /// thirty seconds later.
    @Test func aHalfFilledPairIsRefused() {
        #expect(model("", "993", "smtp.x.jp", "465").manualEndpoints() == nil)
        #expect(model("imap.x.jp", "", "smtp.x.jp", "465").manualEndpoints() == nil)
        #expect(model("imap.x.jp", "993", "", "465").manualEndpoints() == nil)
    }

    /// Digits only: `+993` and `99.5` are not what somebody typing a
    /// port means, and a zero port is not a port.
    @Test func onlyDigitsCountAsAPort() {
        for p in ["+993", "99.5", "abc", "9 9", "0", "70000"] {
            #expect(model("h", p, "s", "465").manualEndpoints() == nil, "port \(p) was accepted")
        }
    }

    /// Spaces around a port are somebody's paste, not a mistake.
    @Test func spacesAroundAPortAreTrimmed() {
        #expect(model("h", " 993 ", "s", "465").manualEndpoints()?.imapPort == 993)
    }

    /// Asking about "s", "so", "som" is three answers nobody asked for.
    @Test func aPartialAddressIsNotLookedUp() {
        var d = MailAccountsModel.Draft()
        for partial in ["s", "so", "some@", "some@x"] {
            d.address = partial
            #expect(!d.addressLooksComplete, "\(partial) was treated as an address")
        }
        d.address = "some@x.jp"
        #expect(d.addressLooksComplete)
    }

    /// The provider's own word, because a person will be looking for
    /// exactly that string in its settings.
    @Test func theSecretIsLabelledWithTheProvidersOwnWord() {
        var d = MailAccountsModel.Draft()
        d.address = "someone@qq.com"
        #expect(d.secretLabel == "授权码")
        d.address = "someone@internal.example.jp"
        #expect(d.secretLabel == "Password")
    }
}

/// What an unknown domain is told.
///
/// The guess is shown rather than described: saying "the usual names
/// are filled in below" while the boxes are shut is a sentence about
/// something the person cannot see, and if the guess is wrong they
/// find out thirty seconds later from a connection failure instead of
/// now, from reading it.
@Suite struct UnknownDomainTests {
    @Test func anUnknownDomainHasAGuessToShow() {
        let g = MailProvider.guess(forDomain: "internal.example.jp")
        #expect(g.imapHost == "imap.internal.example.jp")
        #expect(g.imapPort == 993)
        #expect(g.smtpHost == "smtp.internal.example.jp")
        #expect(g.smtpPort == 465)
    }

    /// A screen that shows one host and connects to another is worse
    /// than one that shows nothing.
    @Test func whatIsShownIsWhatWillBeTried() {
        let shown = MailProvider.guess(forDomain: "internal.example.jp")
        let built = MailAccount.make(address: "me@internal.example.jp")
        #expect(shown.imapHost == built.imapHost)
        #expect(shown.imapPort == built.imapPort)
        #expect(shown.smtpHost == built.smtpHost)
        #expect(shown.smtpPort == built.smtpPort)
    }
}
