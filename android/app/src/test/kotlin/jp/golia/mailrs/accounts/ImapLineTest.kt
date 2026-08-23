package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Reading what an IMAP server says.
 *
 * The same cases as the iOS suite, deliberately: a client that
 * disagrees with itself across platforms is two clients.
 */
class ImapLineTest {
    @Test
    fun `a tagged reply is recognised`() {
        assertEquals(
            Imap.Completion.Ok("LOGIN completed"),
            Imap.completion("a1 OK LOGIN completed", "a1"),
        )
        assertEquals(
            Imap.Completion.No("[AUTHENTICATIONFAILED] bad"),
            Imap.completion("a1 NO [AUTHENTICATIONFAILED] bad", "a1"),
        )
        assertEquals(Imap.Completion.Bad("syntax"), Imap.completion("a1 BAD syntax", "a1"))
    }

    // `a1` must not match `a10`: a server may interleave replies, and a
    // prefix match reads another command's answer as this one's.
    @Test
    fun `a tag is not a prefix of another`() {
        assertNull(Imap.completion("a10 OK done", "a1"))
        assertNull(Imap.completion("a1 OK done", "a10"))
    }

    @Test
    fun `an untagged line is not read as a completion`() {
        assertNull(Imap.completion("* OK ready", "a1"))
        assertNull(Imap.untagged("a1 OK done"))
    }

    // The name is last, quoted, and holds both a space and the
    // delimiter — which is why it is taken from the end.
    @Test
    fun `a gmail folder name survives`() {
        val f = Imap.untagged("""* LIST (\HasNoChildren \Sent) "/" "[Gmail]/Sent Mail"""")
            as Imap.Untagged.ListFolder
        assertEquals("[Gmail]/Sent Mail", f.name)
        assertTrue(f.attributes.contains("\\Sent"))
    }

    @Test
    fun `an unquoted name is read`() {
        val f = Imap.untagged("""* LIST (\HasNoChildren) "." INBOX""") as Imap.Untagged.ListFolder
        assertEquals("INBOX", f.name)
    }

    @Test
    fun `an escaped quote inside a name survives`() {
        val f = Imap.untagged("""* LIST () "/" "od\"d"""") as Imap.Untagged.ListFolder
        assertEquals("""od"d""", f.name)
    }

    @Test
    fun `the count and the validity are read`() {
        assertEquals(Imap.Untagged.Exists(42), Imap.untagged("* 42 EXISTS"))
        assertEquals(
            Imap.Untagged.UidValidity(1234),
            Imap.untagged("* OK [UIDVALIDITY 1234] Ready"),
        )
        assertEquals(Imap.Untagged.UidNext(4391), Imap.untagged("* OK [UIDNEXT 4391] Predicted"))
    }

    // Free text after the code may contain anything, including
    // something that looks like another number.
    @Test
    fun `text after the code is not read as the value`() {
        assertEquals(
            Imap.Untagged.UidValidity(7),
            Imap.untagged("* OK [UIDVALIDITY 7] 99 messages"),
        )
    }

    // Generated app passwords contain `"` and `\` often enough that an
    // unquoted argument turns one into a syntax error.
    @Test
    fun `a password with a quote is escaped`() {
        assertEquals("\"pa\\\"ss\\\\word\"", Imap.quoted("pa\"ss\\word"))
    }

    @Test
    fun `a refused credential is told from a server having a bad day`() {
        assertTrue(Imap.isAuthenticationFailure("[AUTHENTICATIONFAILED] Invalid credentials"))
        assertTrue(Imap.isAuthenticationFailure("LOGIN failed"))
        assertFalse(Imap.isAuthenticationFailure("[UNAVAILABLE] System error"))
        assertFalse(Imap.isAuthenticationFailure("Temporary failure, try again"))
    }
}

/**
 * Reading a `FETCH` line, which is where a mail client truncates
 * somebody's message if it gets the literal wrong.
 */
class ImapFetchTest {
    @Test
    fun `a fetch line carries the uid and the flag`() {
        val a = Imap.fetchLine("""* 12 FETCH (UID 4390 FLAGS (\Seen \Answered) BODY[] {2048}""")
        assertEquals(4390L, a?.uid)
        assertEquals(true, a?.seen)
        assertEquals(2048, a?.literalBytes)
    }

    @Test
    fun `an unread message says so`() {
        val a = Imap.fetchLine("""* 13 FETCH (UID 4391 FLAGS () BODY[] {10}""")
        assertEquals(false, a?.seen)
        assertEquals(4391L, a?.uid)
    }

    // A folder called "Seen" in the same line must not set the flag —
    // the backslash is what makes it a flag rather than a word.
    @Test
    fun `a word that looks like the flag does not set it`() {
        assertEquals(false, Imap.fetchLine("""* 14 FETCH (UID 1 FLAGS () BODY[HEADER] {4}""")?.seen)
    }

    // **The byte count, not a scan.** A message body contains every
    // byte sequence a terminator could be made of, so a client that
    // scans truncates mail at whatever looks like the end.
    @Test
    fun `the literal is a byte count`() {
        assertEquals(0, Imap.fetchLine("""* 1 FETCH (UID 2 BODY[] {0}""")?.literalBytes)
        assertEquals(
            1048576,
            Imap.fetchLine("""* 1 FETCH (UID 2 BODY[] {1048576}""")?.literalBytes,
        )
    }

    // A FETCH with no literal is a flags-only reply, which is a normal
    // thing for a server to send.
    @Test
    fun `a fetch with no literal is still a fetch`() {
        val a = Imap.fetchLine("""* 15 FETCH (UID 7 FLAGS (\Seen))""")
        assertEquals(7L, a?.uid)
        assertEquals(true, a?.seen)
        assertNull(a?.literalBytes)
    }

    @Test
    fun `a line that is not a fetch is not guessed at`() {
        assertNull(Imap.fetchLine("* 12 EXISTS"))
        assertNull(Imap.fetchLine("a1 OK done"))
    }
}
