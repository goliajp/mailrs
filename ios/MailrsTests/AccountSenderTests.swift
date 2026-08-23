import Testing

@testable import Mailrs

/// The decisions in sending that need no server.
@Suite struct AccountSenderTests {
    private func account(_ address: String) -> MailAccount {
        var a = MailAccount.make(address: address, displayName: "", sort: 0)
        a.smtpHost = "smtp.example.com"
        a.smtpPort = 587
        return a
    }

    /// A Message-ID pointing at a domain that has nothing to do with
    /// the sender is one of the things spam filters count.
    @Test func theMessageIdentityUsesTheSendersOwnDomain() {
        let id = AccountSender.identity(for: account("me@example.com"), uuid: "ABC-123")
        #expect(id == "abc-123@example.com")
    }

    /// A HELO naming somebody's phone is refused by a fair number of
    /// servers and greylisted by more.
    @Test func theGreetingNamesADomain() {
        #expect(AccountSender.helo(for: account("me@example.com")) == "example.com")
        #expect(AccountSender.helo(for: account("nonsense")) == "localhost")
    }

    /// 4xx is the moment's fault and 5xx is the message's. Somebody
    /// told "try again" about a permanent refusal will try forever.
    @Test func aTemporaryRefusalSaysTryAgainAndAPermanentOneDoesNot() {
        let temporary = AccountSender.explain(
            .rejected(code: 451, text: "busy", permanent: false))
        #expect(temporary.contains("try again"))

        let permanent = AccountSender.explain(
            .rejected(code: 550, text: "no such user", permanent: true))
        #expect(!permanent.contains("try again"))
        #expect(permanent.contains("no such user"))
    }

    /// The one refusal a person can actually do something about gets
    /// its own sentence.
    @Test func aRefusedSignInSaysSo() {
        let out = AccountSender.explain(.rejected(code: 535, text: "auth failed", permanent: true))
        #expect(out.contains("sign-in"))
    }

    /// Every failure produces a sentence — an empty message is a
    /// screen that says nothing went wrong while nothing was sent.
    @Test func everyFailureSaysSomething() {
        let all: [SMTPSession.Failure] = [
            .unreachable("nw error 61"), .refused("bad greeting"), .closed,
            .rejected(code: 421, text: "", permanent: false),
        ]
        for failure in all {
            #expect(!AccountSender.explain(failure).isEmpty)
        }
    }
}
