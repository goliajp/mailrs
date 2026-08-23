package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Reading a JMAP session object and a changes reply. */
class JmapTest {
    // `primaryAccounts` is what names the mail account.
    @Test
    fun `the mail account comes from primary accounts`() {
        val s = Jmap.session(
            """
            {"apiUrl":"https://api.example.com/jmap",
             "primaryAccounts":{"urn:ietf:params:jmap:mail":"u42"},
             "accounts":{"u1":{},"u42":{}}}
            """.trimIndent(),
        )
        assertEquals(Jmap.Session("https://api.example.com/jmap", "u42"), s)
    }

    // Picking the first key of `accounts` works until somebody has two
    // — and then it silently reads the wrong mailbox.
    @Test
    fun `two accounts with no primary is refused rather than guessed`() {
        assertNull(
            Jmap.session(
                """{"apiUrl":"https://api.example.com/jmap","accounts":{"u1":{},"u2":{}}}""",
            ),
        )
    }

    @Test
    fun `one account needs no primary`() {
        assertEquals(
            "only",
            Jmap.session(
                """{"apiUrl":"https://api.example.com/jmap","accounts":{"only":{}}}""",
            )?.accountId,
        )
    }

    @Test
    fun `a session with no api url is not a session`() {
        assertNull(Jmap.session("""{"accounts":{"u1":{}}}"""))
        assertNull(Jmap.session("""{"apiUrl":""}"""))
        assertNull(Jmap.session("not json"))
    }

    @Test
    fun `changes carry the new state and what arrived`() {
        val c = Jmap.changes(
            """
            {"methodResponses":[["Email/changes",
              {"created":["m1","m2"],"newState":"s2"},"c0"]]}
            """.trimIndent(),
        )
        assertEquals(Jmap.Changes.Some(listOf("m1", "m2"), "s2"), c)
    }

    // **Not an error.** RFC 8620 5.2 tells the client to start over;
    // treating it as a failure leaves an account that never syncs
    // again.
    @Test
    fun `cannot calculate changes means start over not fail`() {
        assertEquals(
            Jmap.Changes.StartOver,
            Jmap.changes(
                """{"methodResponses":[["error",{"type":"cannotCalculateChanges"},"c0"]]}""",
            ),
        )
    }

    // Any other error is a failure and must not be read as a fresh
    // start — that would silently re-download the mailbox.
    @Test
    fun `another error is not a fresh start`() {
        assertNull(
            Jmap.changes("""{"methodResponses":[["error",{"type":"accountNotFound"},"c0"]]}"""),
        )
    }

    @Test
    fun `a reply with no state is not usable`() {
        assertNull(Jmap.changes("""{"methodResponses":[["Email/changes",{},"c0"]]}"""))
        assertNull(Jmap.changes("""{"methodResponses":[]}"""))
    }
}
