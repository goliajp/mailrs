import Testing

@testable import Mailrs

/// Reading what an IMAP server says.
///
/// Everything worth getting wrong here is in the parsing, so it is
/// tested without a socket. The cases are the ones that bite: a
/// mailbox name with spaces and the delimiter inside it, a tag that is
/// a prefix of another tag, and a password with a quote in it.
@Suite struct IMAPLineTests {
    @Test func aTaggedOkIsRecognised() {
        #expect(IMAP.completion(of: "a1 OK LOGIN completed", tag: "a1") == .ok("LOGIN completed"))
        #expect(IMAP.completion(of: "a1 NO [AUTHENTICATIONFAILED] bad", tag: "a1")
            == .no("[AUTHENTICATIONFAILED] bad"))
        #expect(IMAP.completion(of: "a1 BAD syntax", tag: "a1") == .bad("syntax"))
    }

    /// `a1` must not match `a10`. A server may interleave replies, and
    /// a prefix match reads another command's answer as this one's.
    @Test func aTagIsNotAPrefixOfAnother() {
        #expect(IMAP.completion(of: "a10 OK done", tag: "a1") == nil)
        #expect(IMAP.completion(of: "a1 OK done", tag: "a10") == nil)
    }

    @Test func anUntaggedLineThatIsNotOursIsNotMisread() {
        #expect(IMAP.completion(of: "* OK ready", tag: "a1") == nil)
        #expect(IMAP.untagged("a1 OK done") == nil)
    }

    /// The name is last, quoted, and holds both a space and the
    /// delimiter — which is why it is taken from the end rather than
    /// by splitting on spaces.
    @Test func aGmailFolderNameSurvives() {
        let line = #"* LIST (\HasNoChildren \Sent) "/" "[Gmail]/Sent Mail""#
        guard case let .list(name, attributes)? = IMAP.untagged(line) else {
            Issue.record("not parsed as a LIST")
            return
        }
        #expect(name == "[Gmail]/Sent Mail")
        #expect(attributes.contains("\\Sent"))
    }

    @Test func anUnquotedNameIsRead() {
        guard case let .list(name, _)? = IMAP.untagged(#"* LIST (\HasNoChildren) "." INBOX"#) else {
            Issue.record("not parsed")
            return
        }
        #expect(name == "INBOX")
    }

    /// A name may hold a quote of its own.
    @Test func anEscapedQuoteInsideANameSurvives() {
        let line = #"* LIST () "/" "od\"d""#
        guard case let .list(name, _)? = IMAP.untagged(line) else {
            Issue.record("not parsed")
            return
        }
        #expect(name == #"od"d"#)
    }

    @Test func theCountAndTheValidityAreRead() {
        #expect(IMAP.untagged("* 42 EXISTS") == .exists(42))
        #expect(IMAP.untagged("* OK [UIDVALIDITY 1234] Ready") == .uidValidity(1234))
        #expect(IMAP.untagged("* OK [UIDNEXT 4391] Predicted") == .uidNext(4391))
    }

    /// Free text after the code may contain anything, including
    /// something that looks like another number.
    @Test func textAfterTheCodeIsNotReadAsTheValue() {
        #expect(IMAP.untagged("* OK [UIDVALIDITY 7] 99 messages") == .uidValidity(7))
    }

    /// Generated app passwords contain `"` and `\` often enough that
    /// an unquoted argument turns one into a syntax error — and the
    /// person is told their password is wrong when it is right.
    @Test func aPasswordWithAQuoteIsEscaped() {
        #expect(IMAP.quoted(#"pa"ss\word"#) == #""pa\"ss\\word""#)
    }

    /// One is a button to press, the other is waiting.
    @Test func aRefusedCredentialIsToldFromAServerHavingABadDay() {
        #expect(IMAP.isAuthenticationFailure("[AUTHENTICATIONFAILED] Invalid credentials"))
        #expect(IMAP.isAuthenticationFailure("LOGIN failed"))
        #expect(!IMAP.isAuthenticationFailure("[UNAVAILABLE] System error"))
        #expect(!IMAP.isAuthenticationFailure("Temporary failure, try again"))
    }
}

/// Reading a `FETCH` line, which is where a mail client truncates
/// somebody's message if it gets the literal wrong.
@Suite struct IMAPFetchTests {
    @Test func aFetchLineCarriesTheUidAndTheFlag() {
        let line = #"* 12 FETCH (UID 4390 FLAGS (\Seen \Answered) BODY[] {2048}"#
        let a = IMAP.fetchLine(line)
        #expect(a?.uid == 4390)
        #expect(a?.seen == true)
        #expect(a?.literalBytes == 2048)
    }

    @Test func anUnreadMessageSaysSo() {
        let a = IMAP.fetchLine(#"* 13 FETCH (UID 4391 FLAGS () BODY[] {10}"#)
        #expect(a?.seen == false)
        #expect(a?.uid == 4391)
    }

    /// A folder called "Seen" in the same line must not set the flag —
    /// the backslash is what makes it a flag rather than a word.
    @Test func aWordThatLooksLikeTheFlagDoesNotSetIt() {
        let a = IMAP.fetchLine(#"* 14 FETCH (UID 1 FLAGS () BODY[HEADER] {4}"#)
        #expect(a?.seen == false)
    }

    /// **The byte count, not a scan.** A message body contains every
    /// byte sequence a terminator could be made of, so a client that
    /// scans truncates mail at whatever looks like the end.
    @Test func theLiteralIsAByteCount() {
        #expect(IMAP.fetchLine(#"* 1 FETCH (UID 2 BODY[] {0}"#)?.literalBytes == 0)
        #expect(IMAP.fetchLine(#"* 1 FETCH (UID 2 BODY[] {1048576}"#)?.literalBytes == 1_048_576)
    }

    /// A FETCH with no literal is a flags-only reply, which is a
    /// normal thing for a server to send.
    @Test func aFetchWithNoLiteralIsStillAFetch() {
        let a = IMAP.fetchLine(#"* 15 FETCH (UID 7 FLAGS (\Seen))"#)
        #expect(a?.uid == 7)
        #expect(a?.seen == true)
        #expect(a?.literalBytes == nil)
    }

    @Test func aLineThatIsNotAFetchIsNotGuessedAt() {
        #expect(IMAP.fetchLine("* 12 EXISTS") == nil)
        #expect(IMAP.fetchLine("a1 OK done") == nil)
    }
}
