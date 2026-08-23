import Foundation
import Testing

@testable import Mailrs

/// Which address a message leaves by.
///
/// The rule is the same on all three clients and each has its own copy
/// of this test, because getting it wrong is invisible: the message
/// sends, and lands in the conversation as a stranger — and half the
/// time the recipient's provider refuses it outright.
@Suite struct ReplyFromTests {
    /// Built from JSON rather than memberwise: the struct decodes
    /// itself by hand, which removes the synthesised initialiser — and
    /// going through the decoder is the path production takes anyway.
    private func account(
        _ id: String, _ email: String, state: String = "ok", name: String = ""
    ) -> Wire.ExternalAccount {
        let raw = """
            {"id":"\(id)","email":"\(email)","display_name":"\(name)",
             "provider":"gmail","state":"\(state)"}
            """
        return try! JSONDecoder().decode(
            Wire.ExternalAccount.self, from: Data(raw.utf8))
    }

    @Test func thisServersOwnAddressComesFirst() {
        let out = fromAddresses(own: "me@golia.jp", accounts: [account("a1", "me@gmail.com")])
        #expect(out.first?.address == "me@golia.jp")
        #expect(out.first?.accountId == "")
    }

    /// Choosing it would produce a message that cannot be sent, and
    /// offering a choice that fails is worse than not offering it.
    @Test func anAccountWhoseCredentialWasRefusedIsNotOffered() {
        let out = fromAddresses(
            own: "me@golia.jp", accounts: [account("a1", "x@gmail.com", state: "needs_auth")])
        #expect(out.count == 1)
    }

    @Test func aNamedAccountShowsItsNameBesideTheAddress() {
        let out = fromAddresses(
            own: "me@golia.jp", accounts: [account("a1", "x@gmail.com", name: "Work")])
        #expect(out[1].label == "Work · x@gmail.com")
    }

    @Test func aNameEqualToTheAddressIsNotRepeated() {
        let out = fromAddresses(
            own: "me@golia.jp", accounts: [account("a1", "x@gmail.com", name: "x@gmail.com")])
        #expect(out[1].label == "x@gmail.com")
    }

    /// A reply to mail that arrived at a connected Gmail goes out
    /// through that Gmail.
    @Test func aReplyFollowsTheAccountTheMailArrivedAt() {
        let addresses = fromAddresses(own: "me@golia.jp", accounts: [account("a1", "x@gmail.com")])
        #expect(replyFromFor("a1", addresses: addresses) == "x@gmail.com")
    }

    @Test func mailThatArrivedHereLeavesFromHere() {
        let addresses = fromAddresses(own: "me@golia.jp", accounts: [account("a1", "x@gmail.com")])
        #expect(replyFromFor("", addresses: addresses) == "me@golia.jp")
        #expect(replyFromFor(nil, addresses: addresses) == "me@golia.jp")
    }

    /// Replying from somewhere beats a composer that will not send.
    @Test func anAccountThatIsGoneFallsBackRatherThanRefusing() {
        let addresses = fromAddresses(own: "me@golia.jp", accounts: [account("a1", "x@gmail.com")])
        #expect(replyFromFor("deleted", addresses: addresses) == "me@golia.jp")
    }
}

/// The second line of an account row.
///
/// An account with no name of its own falls back to the address on the
/// first line, so repeating it underneath says nothing — and the row
/// carried the same text twice, which the Android instrumentation
/// suite reported as "found 2 nodes".
@Suite struct AccountSubtitleTests {
    @Test func aNamedAccountShowsItsAddressUnderneath() {
        #expect(accountSubtitle(displayName: "Work", email: "x@gmail.com") == "x@gmail.com")
    }

    @Test func anUnnamedAccountDoesNotRepeatItself() {
        #expect(accountSubtitle(displayName: "", email: "x@gmail.com") == nil)
    }

    @Test func aNameEqualToTheAddressDoesNotRepeatItselfEither() {
        #expect(accountSubtitle(displayName: "x@gmail.com", email: "x@gmail.com") == nil)
    }
}
