import Testing

@testable import Mailrs

/// Mirrors `web/src/lib/__tests__/avatar.test.ts` case for case — the
/// two clients must put the same face on the same mail.
struct SenderNameTests {
    @Test func extractsTheAddressFromEveryShape() {
        #expect(SenderName.extractEmail("Alice Smith <alice@example.com>") == "alice@example.com")
        #expect(SenderName.extractEmail("<bob@example.com>") == "bob@example.com")
        #expect(SenderName.extractEmail("charlie@example.com") == "charlie@example.com")
        #expect(SenderName.extractEmail("not-an-email") == "not-an-email")
        #expect(SenderName.extractEmail("\"Dave Doe\" <dave@example.com>") == "dave@example.com")
    }

    @Test func extractsTheDisplayName() {
        #expect(SenderName.extractName("Alice Smith <alice@example.com>") == "Alice Smith")
        #expect(SenderName.extractName("\"Bob Jones\" <bob@example.com>") == "Bob Jones")
        #expect(SenderName.extractName("  Dave  <dave@example.com>") == "Dave")
    }

    @Test func bareAddressesShowTheLocalPart() {
        #expect(SenderName.extractName("charlie@example.com") == "charlie")
        #expect(SenderName.extractName("notanemail") == "notanemail")
    }

    /// A tracking-id local part is not a name; the brand from the domain
    /// is — `notify.cloudflare.com` reads Cloudflare, `em8742.bsm.freee
    /// .work` reads Freee.
    @Test func machineAddressesShowTheBrand() {
        #expect(SenderName.extractName("bounce-12345-abcdef@notify.cloudflare.com") == "Cloudflare")
        #expect(SenderName.extractName("msprvs1=18274xyzw=k@em8742.bsm.freee.work") == "Freee")
    }

    /// RFC 2047 encoded-words — the shape every Japanese sender uses.
    @Test func decodesEncodedWordNames() {
        // "山田太郎" base64/UTF-8
        let encoded = "=?UTF-8?B?5bGx55Sw5aSq6YOO?= <yamada@example.co.jp>"
        #expect(SenderName.extractName(encoded) == "山田太郎")
        #expect(SenderName.extractEmail(encoded) == "yamada@example.co.jp")
    }

    @Test func decodesQuotedPrintableNames() {
        #expect(SenderName.decodeMimeHeader("=?UTF-8?Q?Caf=C3=A9?=") == "Café")
    }

    @Test func domainLabelSkipsSecondaryTlds() {
        #expect(SenderName.domainLabel("example.co.jp") == "example")
        #expect(SenderName.domainLabel("notify.cloudflare.com") == "cloudflare")
        #expect(SenderName.domainLabel("localhost") == "localhost")
    }
}

/// The row-face rule, separate from name extraction: whose name does a
/// conversation row wear.
struct RowFaceTests {
    @Test func theFaceIsTheOtherParticipant() {
        let face = SenderName.rowFace(
            participants: ["me@golia.jp", "Alice Smith <alice@example.com>"],
            myAddress: "me@golia.jp"
        )
        #expect(face == "Alice Smith")
    }

    /// The web's own-reply bug, kept fixed: my address at [0] must not
    /// become the row's face.
    @Test func ownReplyDoesNotStealTheFace() {
        let face = SenderName.rowFace(
            participants: ["Me <ME@Golia.jp>", "bob@example.com"],
            myAddress: "me@golia.jp"
        )
        #expect(face == "bob")
    }

    @Test func selfOnlyThreadSaysMe() {
        let face = SenderName.rowFace(participants: ["me@golia.jp"], myAddress: "me@golia.jp")
        #expect(face == "Me")
    }

    @Test func emptyParticipantsStayUnknown() {
        #expect(SenderName.rowFace(participants: [], myAddress: "me@golia.jp") == "(unknown)")
    }
}
