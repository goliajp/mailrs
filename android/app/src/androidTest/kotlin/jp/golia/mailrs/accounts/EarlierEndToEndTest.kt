package jp.golia.mailrs.accounts

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Fetching the mail before what is held.
 *
 * The rules that cost something when they are wrong: a range that is
 * all gaps must not become "there is nothing older", and a folder that
 * was renumbered while the question was in flight must not have
 * whatever now sits at those uids read as the answer.
 */
@RunWith(AndroidJUnit4::class)
class EarlierEndToEndTest {
    private lateinit var store: AccountStore
    private val account = MailAccount.make("me@example.com", "Me", 0).copy(
        imapHost = "imap.example.com",
        imapPort = 993,
    )

    private class Script(private val lines: MutableList<String>) : ImapSession.Transport {
        val written = mutableListOf<String>()
        private var pending = StringBuilder()
        override fun readLine(): String {
            if (pending.isNotEmpty()) {
                val out = pending.toString()
                pending = StringBuilder()
                return out
            }
            if (lines.isEmpty()) throw ImapSession.Failure.Closed()
            return lines.removeAt(0)
        }
        override fun readBytes(count: Int): String {
            val out = StringBuilder()
            while (out.length < count) {
                if (lines.isEmpty()) throw ImapSession.Failure.Closed()
                out.append(lines.removeAt(0))
            }
            if (out.length > count) pending = StringBuilder(out.substring(count))
            return out.substring(0, count)
        }
        override fun write(text: String) {
            written.add(text.trimEnd('\r', '\n'))
        }
        override fun close() = Unit
    }

    private fun serving(vararg lines: String): Script {
        val script = Script(lines.toMutableList())
        MailboxSyncRunner.openImap = { _, _ ->
            ImapSession("localhost", 993).also { it.transport = script }
        }
        return script
    }

    @Before
    fun setUp() {
        store = AccountStore(ApplicationProvider.getApplicationContext())
        store.save(listOf(account))
        store.saveSecret("app-password", account.id)
        store.saveRows(emptyList())
        store.saveMarksFor(account.id, mapOf("INBOX" to FolderMark(42L, 1200L, 1001L, 200)))
    }

    @After
    fun tearDown() {
        MailboxSyncRunner.openImap = { host, port -> ImapSession(host, port) }
        store.remove(account.id)
        store.saveRows(emptyList())
    }

    private fun header(subject: String) =
        "From: Ada <ada@example.com>\r\nSubject: $subject\r\n" +
            "Date: Sun, 24 Aug 2025 01:46:40 +0000\r\nMessage-ID: <$subject@example.com>\r\n\r\n"

    /** It asks for the span below what is held, and keeps what comes. */
    @Test
    fun it_reaches_below_the_lowest_held() = runBlocking {
        val body = header("older")
        val script = serving(
            "* OK ready",
            "a1 OK signed in",
            "* OK [UIDVALIDITY 42] valid",
            "a2 OK selected",
            "* 1 FETCH (UID 900 FLAGS () BODY[HEADER] {" + body.length + "}",
            body + ")",
            "a3 OK fetched",
        )
        val outcome = MailboxSyncRunner.earlier(account, "INBOX", store)
        assertEquals(outcome.failure, null, outcome.failure)
        assertEquals(1, outcome.fetched)
        val fetch = script.written.first { it.contains("UID FETCH") }
        assertTrue(fetch, fetch.contains("801:1000"))
        assertEquals(900L, store.rows().single().uid)
    }

    /**
     * **A range that is all gaps is not the end of the folder.** It
     * returns nothing, and there may be plenty below it — so the next
     * ask starts from the range that was tried, not from what came
     * back, and it asks wider.
     */
    @Test
    fun an_empty_range_moves_on_and_widens() = runBlocking {
        serving("* OK ready", "a1 OK signed in", "* OK [UIDVALIDITY 42] valid", "a2 OK selected", "a3 OK fetched")
        val outcome = MailboxSyncRunner.earlier(account, "INBOX", store)
        assertEquals(0, outcome.fetched)
        assertEquals(outcome.failure, null, outcome.failure)
        val mark = store.marksFor(account.id)["INBOX"]!!
        assertEquals("the next ask would repeat the empty one", 801L, mark.lowestUid)
        assertTrue("the span did not widen", mark.earlierSpan > 200)
    }

    /**
     * **A renumbered folder means every held uid points at something
     * else.** Reaching below one of them would fetch whatever now sits
     * there, and file it as older mail.
     */
    @Test
    fun a_renumbered_folder_is_refused_rather_than_read() = runBlocking {
        serving("* OK ready", "a1 OK signed in", "* OK [UIDVALIDITY 99] valid", "a2 OK selected")
        val outcome = MailboxSyncRunner.earlier(account, "INBOX", store)
        assertTrue(outcome.failure.orEmpty(), outcome.failure != null)
        assertTrue(store.rows().isEmpty())
    }

    /** Holding uid 1 means there is nothing older, and nothing is asked. */
    @Test
    fun the_beginning_of_the_folder_asks_nothing() = runBlocking {
        store.saveMarksFor(account.id, mapOf("INBOX" to FolderMark(42L, 1200L, 1L, 200)))
        val script = serving("* OK ready")
        val outcome = MailboxSyncRunner.earlier(account, "INBOX", store)
        assertEquals(0, outcome.fetched)
        assertEquals(outcome.failure, null, outcome.failure)
        assertTrue("a round trip was made for an answer already known", script.written.isEmpty())
    }
}
