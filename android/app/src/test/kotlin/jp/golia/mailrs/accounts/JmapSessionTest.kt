package jp.golia.mailrs.accounts

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/** Asking a JMAP server, without one. */
class JmapSessionTest {
    private class FakeHttp(private val answers: MutableList<Pair<Int, String>>) : JmapSession.Http {
        val asked = mutableListOf<Triple<String, String, String?>>()
        override fun post(url: String, authorization: String, body: String?): Pair<Int, String> {
            asked.add(Triple(url, authorization, body))
            if (answers.isEmpty()) return 500 to ""
            return answers.removeAt(0)
        }
    }

    private fun session(vararg answers: Pair<Int, String>): Pair<JmapSession, FakeHttp> {
        val fake = FakeHttp(answers.toMutableList())
        val s = JmapSession("mail.example.com")
        s.http = fake
        return s to fake
    }

    /**
     * Sending a token as a password is refused by every server that
     * issues tokens — and the person is then told their password is
     * wrong for an account whose credentials are fine.
     */
    @Test
    fun `a token is a bearer and a password is basic`() {
        val (s, _) = session()
        assertEquals("Bearer tok-123", s.authorization("", "tok-123"))
        val basic = s.authorization("me@example.com", "secret")
        assertTrue(basic, basic.startsWith("Basic "))
        val decoded = String(
            java.util.Base64.getDecoder().decode(basic.removePrefix("Basic ")),
            Charsets.UTF_8,
        )
        assertEquals("me@example.com:secret", decoded)
    }

    /** `/.well-known/jmap` is the only entry point a client may assume. */
    @Test
    fun `the session is asked for at the well known place`() = runBlocking {
        val body = """{"apiUrl":"https://api.example.com/jmap",
            "primaryAccounts":{"urn:ietf:params:jmap:mail":"acct-9"}}"""
        val (s, http) = session(200 to body)
        val found = s.session("me@example.com", "secret")
        assertEquals("https://mail.example.com/.well-known/jmap", http.asked[0].first)
        assertEquals("acct-9", found.accountId)
        assertEquals("https://api.example.com/jmap", found.apiUrl)
        // A GET, not a POST: nothing is being sent.
        assertEquals(null, http.asked[0].third)
    }

    /**
     * A refused credential is a refusal, not a server fault — the two
     * lead a person to do completely different things.
     */
    @Test
    fun `a 401 is a refused credential`() = runBlocking {
        val (s, _) = session(401 to """{"type":"unauthorized"}""")
        try {
            s.session("me@example.com", "wrong")
            fail("a refused credential was reported as a session")
        } catch (e: JmapSession.Failure.Refused) {
            assertTrue(e.detail, e.detail.contains("credential"))
        }
    }

    /** And a server that is simply broken says so as a server fault. */
    @Test
    fun `a 500 is a server fault`() = runBlocking {
        val (s, _) = session(500 to "boom")
        try {
            s.session("me@example.com", "secret")
            fail("a broken server was reported as a session")
        } catch (e: JmapSession.Failure.Server) {
            assertTrue(e.detail, e.detail.contains("500"))
        }
    }

    /** The mail request goes to the api url the session named. */
    @Test
    fun `mail is asked for at the api url`() = runBlocking {
        val reply = """{"methodResponses":[["Email/get",{"list":[
            {"id":"m1","subject":"hi","from":[{"email":"a@b.com"}],
             "receivedAt":"2025-08-24T01:46:40Z","keywords":{}}]},"1"]]}"""
        val (s, http) = session(200 to reply)
        val found = Jmap.Session("https://api.example.com/jmap", "acct-9")
        val emails = s.newest(found, "me@example.com", "secret", 10)
        assertEquals("https://api.example.com/jmap", http.asked[0].first)
        assertTrue(http.asked[0].third!!.contains("\"accountId\":\"acct-9\""))
        assertEquals(1, emails.size)
        assertEquals("hi", emails[0].subject)
    }

    /**
     * A session object that does not say which account holds the mail
     * is not a session — guessing there reads somebody else's mailbox.
     */
    @Test
    fun `an ambiguous session is refused rather than guessed`() = runBlocking {
        val body = """{"apiUrl":"https://api.example.com/jmap",
            "accounts":{"a":{},"b":{}}}"""
        val (s, _) = session(200 to body)
        try {
            s.session("me@example.com", "secret")
            fail("an ambiguous session was guessed at")
        } catch (e: JmapSession.Failure.Server) {
            assertTrue(e.detail, e.detail.contains("which account"))
        }
    }
}
