import Testing

@testable import Mailrs

struct DraftRuleTests {
    @Test func doesNotSaveAnEmptyCompose() {
        #expect(!DraftRule.isWorthSaving(to: "", subject: "", body: ""))
        #expect(!DraftRule.isWorthSaving(to: "  ", subject: "\n", body: " \t "))
    }

    /// Any one field is enough — a recipient with nothing written yet is
    /// still someone you meant to write to.
    @Test func savesWhenAnyFieldHasContent() {
        #expect(DraftRule.isWorthSaving(to: "a@b.jp", subject: "", body: ""))
        #expect(DraftRule.isWorthSaving(to: "", subject: "Hello", body: ""))
        #expect(DraftRule.isWorthSaving(to: "", subject: "", body: "note to self"))
    }

    @Test func titlesBySubjectWhenThereIsOne() {
        #expect(DraftRule.title(subject: "Q3 report", body: "anything") == "Q3 report")
    }

    @Test func fallsBackToTheFirstLineOfTheBody() {
        #expect(DraftRule.title(subject: "", body: "first line\nsecond") == "first line")
        #expect(DraftRule.title(subject: "   ", body: "  padded  \nmore") == "padded")
    }

    @Test func saysSoWhenThereIsNothingToTitleWith() {
        #expect(DraftRule.title(subject: "", body: "") == "(no subject)")
        #expect(DraftRule.title(subject: " ", body: "\n\n") == "(no subject)")
    }
}
