package jp.golia.mailrs.accounts

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A JMAP account, wire joined to store.
 *
 * JMAP's shapes are easy to get wrong in ways that produce a plausible
 * empty screen rather than an error: `from` is a list of objects, an
 * absent `$seen` means unread, and the account id lives in
 * `primaryAccounts`. Reading any of them wrongly gives rows that look
 * fine and are not.
 */
@RunWith(AndroidJUnit4::class)
class JmapEndToEndTest {
    private lateinit var store: AccountStore
    private val account = MailAccount.make("me@example.com", "Me", 0).copy(
        imapHost = "mail.example.com",
        imapPort = 443,
        incoming = Incoming.JMAP,
    )

    private class FakeHttp(private val answers: MutableList<Pair<Int, String>>) : JmapSession.Http {
        val asked = mutableListOf<Triple<String, String, String?>>()
        override fun post(url: String, authorization: String, body: String?): Pair<Int, String> {
            asked.add(Triple(url, authorization, body))
            if (answers.isEmpty()) return 500 to ""
            return answers.removeAt(0)
        }
    }

    private fun serving(vararg answers: Pair<Int, String>): FakeHttp {
        val fake = FakeHttp(answers.toMutableList())
        MailboxSyncRunner.openJmap = { JmapSession("mail.example.com").also { it.http = fake } }
        return fake
    }

    @Before
    fun setUp() {
        store = AccountStore(ApplicationProvider.getApplicationContext())
        store.save(listOf(account))
        store.saveSecret("app-password", account.id)
        store.replaceRows(emptyList())
    }

    @After
    fun tearDown() {
        MailboxSyncRunner.openJmap = { host -> JmapSession(host) }
        store.remove(account.id)
        store.replaceRows(emptyList())
    }

    private val session = """
        {"apiUrl":"https://api.example.com/jmap",
         "primaryAccounts":{"urn:ietf:params:jmap:mail":"acct-9"}}
    """.trimIndent()

    private val mail = """
        {"methodResponses":[
          ["Email/query",{"ids":["m1","m2"]},"0"],
          ["Email/get",{"list":[
            {"id":"m1","subject":"会議のお知らせ",
             "from":[{"name":"事務局","email":"office@example.jp"}],
             "receivedAt":"2025-08-24T01:46:40Z",
             "keywords":{},
             "messageId":["<m1@example.jp>"]},
            {"id":"m2","subject":"Read already",
             "from":[{"email":"bob@example.com"}],
             "receivedAt":"2025-08-24T02:00:00Z",
             "keywords":{"${'$'}seen":true},
             "messageId":["<m2@example.com>"]}
          ]},"1"]
        ]}
    """.trimIndent()

    /**
     * One pass, and the three shapes that produce a plausible wrong
     * answer if read wrongly.
     */
    @Test
    fun a_pass_reads_the_shapes_that_are_easy_to_get_wrong() = runBlocking {
        val http = serving(200 to session, 200 to mail)
        val outcome = MailboxSyncRunner.run(account, store)
        assertEquals(outcome.failure, null, outcome.failure)
        assertEquals(2, outcome.fetched)

        // The api url came from the session object, not from a guess.
        assertEquals("https://mail.example.com/.well-known/jmap", http.asked[0].first)
        assertEquals("https://api.example.com/jmap", http.asked[1].first)
        // And the account id from `primaryAccounts`, not the first key
        // of `accounts` — that works until somebody has two.
        assertTrue(http.asked[1].third!!.contains("\"accountId\":\"acct-9\""))
        // One round trip for the list, through the back-reference.
        assertTrue(http.asked[1].third!!.contains("\"#ids\""))
        assertEquals(2, http.asked.size)

        val rows = store.rows().sortedBy { it.subject }
        assertEquals(2, rows.size)
        val meeting = rows.first { it.subject == "会議のお知らせ" }
        // `from` is a list of objects; read as text it empties every row.
        assertEquals("事務局 <office@example.jp>", meeting.sender)
        // `receivedAt` is a UTC string, not a number.
        assertEquals(1_756_000_000L, meeting.date)
        // An absent `${'$'}seen` means unread — the same absence IMAP uses.
        assertFalse(meeting.seen)
        assertTrue(rows.first { it.subject == "Read already" }.seen)
    }

    /**
     * A refused credential says so and changes nothing. Reading
     * `/.well-known/jmap` proves nothing about it — a server hands
     * that to anybody — so the failure has to come from a real
     * request.
     */
    @Test
    fun a_refused_credential_says_so() = runBlocking {
        serving(401 to """{"type":"unauthorized"}""")
        val outcome = MailboxSyncRunner.run(account, store)
        assertTrue("a refused credential was reported as a pass", outcome.failure != null)
        assertTrue(store.rows().isEmpty())
    }

    /**
     * A session object that does not say which account holds the mail
     * is refused rather than guessed — guessing reads somebody else's
     * mailbox.
     */
    @Test
    fun an_ambiguous_session_is_not_guessed_at() = runBlocking {
        val http = serving(
            200 to """{"apiUrl":"https://api.example.com/jmap","accounts":{"a":{},"b":{}}}""",
        )
        val outcome = MailboxSyncRunner.run(account, store)
        assertTrue("an ambiguous session was guessed at", outcome.failure != null)
        // And no mail was asked for at all.
        assertEquals(1, http.asked.size)
        assertTrue(store.rows().isEmpty())
    }
}
