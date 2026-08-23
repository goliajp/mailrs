import Foundation
import Testing

@testable import Mailrs

/// Reading what an SMTP server says, and the two AUTH payloads.
@Suite struct SMTPLineTests {
    /// The fourth character decides whether more lines follow. Getting
    /// it wrong reads the next command's reply as this one's.
    @Test func aContinuationIsToldFromAnEnding() {
        #expect(SMTP.reply("250-STARTTLS")?.more == true)
        #expect(SMTP.reply("250 OK")?.more == false)
        #expect(SMTP.reply("250")?.more == false)
    }

    @Test func aCodeSaysWhetherToTryAgain() {
        #expect(SMTP.reply("451 try later")?.isPermanent == false)
        #expect(SMTP.reply("550 no such user")?.isPermanent == true)
        #expect(SMTP.reply("250 OK")?.isPositive == true)
        #expect(SMTP.reply("550 no")?.isPositive == false)
    }

    @Test func aLineThatIsNotAReplyIsNotGuessedAt() {
        #expect(SMTP.reply("hello") == nil)
        #expect(SMTP.reply("250x OK") == nil)
        #expect(SMTP.reply("") == nil)
    }

    /// NUL separators, not spaces. Spaces authenticate as nobody and
    /// the server answers with what reads as a wrong password.
    @Test func authPlainIsNulSeparated() {
        let raw = Data(base64Encoded: SMTP.authPlain(user: "me@x.com", password: "hunter2"))!
        #expect(Array(raw) == [0] + Array("me@x.com".utf8) + [0] + Array("hunter2".utf8))
    }

    /// The authorisation identity is empty: repeating the username
    /// there is accepted by some servers and refused by Gmail.
    @Test func theAuthorisationIdentityIsEmpty() {
        let raw = Data(base64Encoded: SMTP.authPlain(user: "u", password: "p"))!
        #expect(raw.first == 0)
    }

    /// An access token is not a password, and the difference is the
    /// whole point.
    @Test func anAccessTokenIsNotSentAsAPassword() {
        let plain = SMTP.authPlain(user: "me@gmail.com", password: "ya29.token")
        let xoauth = SMTP.authXOAuth2(user: "me@gmail.com", token: "ya29.token")
        #expect(plain != xoauth)
        let raw = Data(base64Encoded: xoauth)!
        #expect(String(decoding: raw, as: UTF8.self)
            == "user=me@gmail.com\u{1}auth=Bearer ya29.token\u{1}\u{1}")
        #expect(!raw.contains(0), "a NUL here is the AUTH PLAIN shape, which is refused")
    }

    /// A body line beginning with `.` would end the DATA block,
    /// truncating the message at that line.
    @Test func aLineStartingWithADotDoesNotEndTheMessage() {
        let out = SMTP.dotStuffed("first\n.hidden\nlast")
        #expect(out == "first\r\n..hidden\r\nlast")
    }

    @Test func anOrdinaryBodyIsOnlyGivenCrlf() {
        #expect(SMTP.dotStuffed("a\nb") == "a\r\nb")
        #expect(SMTP.dotStuffed("a\r\nb") == "a\r\nb")
    }

    /// A dot in the middle of a line is not a terminator and must not
    /// be doubled — that would corrupt the text.
    @Test func aDotInsideALineIsLeftAlone() {
        #expect(SMTP.dotStuffed("see fig. 1") == "see fig. 1")
    }

    @Test func aRefusedCredentialIsToldFromAServerHavingABadDay() {
        #expect(SMTP.isAuthenticationFailure(code: 535, text: "5.7.8 nope"))
        #expect(SMTP.isAuthenticationFailure(code: 501, text: "Username and Password not accepted"))
        #expect(!SMTP.isAuthenticationFailure(code: 451, text: "Temporary system problem"))
    }
}
