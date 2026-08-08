import Testing

@testable import Mailrs

struct AuditActionTests {
    @Test func theFamilyIsWhatTheServerFiltersOn() {
        #expect(AuditAction.family(of: "alias.create") == "alias")
        #expect(AuditAction.family(of: "account.delete") == "account")
    }

    @Test func theVerbIsTheRest() {
        #expect(AuditAction.verb(of: "alias.create") == "create")
        #expect(AuditAction.verb(of: "email_group.member.add") == "member.add")
    }

    /// An action with no dot is its own verb, not an empty one — the
    /// server is free to write a bare word and the screen must still
    /// say something.
    @Test func aBareActionIsBothHalves() {
        #expect(AuditAction.family(of: "login") == "login")
        #expect(AuditAction.verb(of: "login") == "login")
    }

    /// Removals are the rows worth finding in a hurry.
    @Test func removalsAreMarked() {
        #expect(AuditAction.isDestructive("alias.delete"))
        #expect(AuditAction.isDestructive("group.member.remove"))
        #expect(AuditAction.isDestructive("apikey.revoke"))
        #expect(!AuditAction.isDestructive("alias.create"))
        #expect(!AuditAction.isDestructive("account.update"))
    }
}
