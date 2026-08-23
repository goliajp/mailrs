package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Asking a JMAP server for a list, and reading what comes back. */
class JmapEmailTest {
    /**
     * The back-reference is what makes it one round trip. A client
     * without it asks, waits, and asks again — two of everything on a
     * phone, including the latency.
     */
    @Test
    fun `the request feeds the query into the get`() {
        val body = Jmap.newestRequest("acct-1", 25)
        assertTrue(body, body.contains("\"#ids\""))
        assertTrue(body, body.contains("\"resultOf\":\"0\""))
        assertTrue(body, body.contains("\"path\":\"/ids\""))
        assertTrue(body, body.contains("\"limit\":25"))
        assertTrue(body, body.contains("\"accountId\":\"acct-1\""))
        // Newest first, or a limit takes the wrong end of the mailbox.
        assertTrue(body, body.contains("\"isAscending\":false"))
    }

    private val reply = """
        {"methodResponses":[
          ["Email/query",{"ids":["m1","m2"]},"0"],
          ["Email/get",{"list":[
            {"id":"m1","subject":"Lunch",
             "from":[{"name":"Ada","email":"ada@example.com"}],
             "receivedAt":"2025-08-24T01:46:40Z",
             "keywords":{"${'$'}seen":true},
             "messageId":["<m1@example.com>"]},
            {"id":"m2","subject":"",
             "from":[{"email":"noreply@example.com"}],
             "receivedAt":"2025-08-24T02:00:00Z",
             "keywords":{},
             "messageId":[]}
          ]},"1"]
        ]}
    """.trimIndent()

    /** `from` is a list of objects; reading it as text empties every row. */
    @Test
    fun `the sender is read out of its object`() {
        val emails = Jmap.emails(reply)!!
        assertEquals("Ada <ada@example.com>", emails[0].sender)
        // A sender with no display name is its address, not a blank.
        assertEquals("noreply@example.com", emails[1].sender)
    }

    /**
     * `keywords` says what is true, so an absent `${'$'}seen` means unread —
     * the same absence IMAP's flag list uses.
     */
    @Test
    fun `absence of the seen keyword means unread`() {
        val emails = Jmap.emails(reply)!!
        assertTrue(emails[0].seen)
        assertFalse(emails[1].seen)
    }

    /** `receivedAt` is a UTC date string, not a number. */
    @Test
    fun `the received time is read as utc`() {
        assertEquals(1_756_000_000L, Jmap.emails(reply)!![0].receivedAt)
    }

    /**
     * Hand-read rather than handed to a formatter: a formatter brings a
     * locale and a default time zone with it, which is how a message
     * moves by hours for somebody who is not in UTC.
     */
    @Test
    fun `an unreadable date is null and never now`() {
        assertNull(Jmap.utcDate(null))
        assertNull(Jmap.utcDate(""))
        assertNull(Jmap.utcDate("yesterday"))
        assertNull(Jmap.utcDate("2025-08-24 01:46:40Z"))
        assertNull(Jmap.utcDate("2025-13-24T01:46:40Z"))
        assertEquals(0L, Jmap.utcDate("1970-01-01T00:00:00Z"))
    }

    /**
     * A server may answer in any order, and one that puts something in
     * front of the get shifts it — reading position 1 blindly then
     * parses the wrong response.
     */
    @Test
    fun `the get is found by name and not by position`() {
        val shifted = """
            {"methodResponses":[
              ["Core/echo",{},"x"],
              ["Email/query",{"ids":["m1"]},"0"],
              ["Email/get",{"list":[{"id":"m1","subject":"found"}]},"1"]
            ]}
        """.trimIndent()
        assertEquals("found", Jmap.emails(shifted)!![0].subject)
    }

    /** Nonsense is null rather than a crash or an empty list. */
    @Test
    fun `broken input is null`() {
        assertNull(Jmap.emails("not json"))
        assertNull(Jmap.emails("""{"methodResponses":[]}"""))
        assertNull(Jmap.emails("""{"methodResponses":[["Email/query",{},"0"]]}"""))
    }
}
