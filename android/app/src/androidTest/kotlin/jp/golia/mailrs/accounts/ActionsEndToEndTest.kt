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
 * Deleting and marking unread, wire joined to store.
 *
 * The rule under test is an **order**: the row goes from this device
 * only after the server says it has gone from there. A row removed
 * first and a move that then fails is a message somebody cannot see
 * and has not lost — it comes back on the next fetch, looking like a
 * bug rather than like the failure it was. Neither half alone can show
 * that.
 */
@RunWith(AndroidJUnit4::class)
class ActionsEndToEndTest {
    private lateinit var store: AccountStore
    private val account = MailAccount.make("me@example.com", "Me", 0).copy(
        imapHost = "imap.example.com",
        imapPort = 993,
    )
    private val row = MailboxRow(
        accountId = MailAccount.idFor("me@example.com"), uid = 7L, folder = "INBOX",
        seen = false, sender = "Ada", subject = "Lunch", date = null, messageId = "m7",
    )

    private class Script(private val lines: MutableList<String>) : ImapSession.Transport {
        val written = mutableListOf<String>()
        override fun readLine(): String {
            if (lines.isEmpty()) throw ImapSession.Failure.Closed()
            return lines.removeAt(0)
        }
        override fun readBytes(count: Int): String {
            val out = StringBuilder()
            while (out.length < count) {
                if (lines.isEmpty()) throw ImapSession.Failure.Closed()
                out.append(lines.removeAt(0))
            }
            return out.substring(0, count)
        }
        override fun write(text: String) {
            written.add(text.trimEnd('\r', '\n'))
        }
        override fun close() = Unit
    }

    private fun serving(vararg lines: String): Script {
        val script = Script(lines.toMutableList())
        MailboxActions.openImap = { _, _ ->
            ImapSession("localhost", 993).also { it.transport = script }
        }
        return script
    }

    @Before
    fun setUp() {
        store = AccountStore(ApplicationProvider.getApplicationContext())
        store.save(listOf(account))
        store.saveSecret("app-password", account.id)
        store.saveRows(listOf(row))
    }

    @After
    fun tearDown() {
        MailboxActions.openImap = { host, port -> ImapSession(host, port) }
        store.remove(account.id)
        store.saveRows(emptyList())
    }

    /** The whole exchange, and the row gone afterwards. */
    @Test
    fun a_delete_moves_it_and_then_forgets_it() = runBlocking {
        val script = serving(
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in",
            "* LIST (\\HasNoChildren \\Trash) \".\" \"Deleted Items\"",
            "a2 OK listed",
            "a3 OK selected",
            "* CAPABILITY IMAP4rev1 MOVE",
            "a4 OK capabilities",
            "a5 OK moved",
        )
        val outcome = MailboxActions.delete(account, row, store)
        assertTrue(outcome.toString(), outcome is MailboxActions.Outcome.Done)
        // The name came from the server's own `\Trash` marker, not from
        // a guess — this account calls it "Deleted Items".
        assertTrue(
            script.written.toString(),
            script.written.any { it.contains("UID MOVE 7 \"Deleted Items\"") },
        )
        assertTrue("the row survived a delete the server accepted", store.rows().isEmpty())
    }

    /**
     * **And the row stays when the server refuses.** This is the whole
     * point of the order.
     */
    @Test
    fun a_refused_delete_leaves_the_row_alone() = runBlocking {
        serving(
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in",
            "* LIST (\\HasNoChildren \\Trash) \".\" \"Trash\"",
            "a2 OK listed",
            "a3 OK selected",
            "* CAPABILITY IMAP4rev1 MOVE",
            "a4 OK capabilities",
            "a5 NO over quota",
        )
        val outcome = MailboxActions.delete(account, row, store)
        assertTrue(outcome.toString(), outcome is MailboxActions.Outcome.Failed)
        assertEquals(1, store.rows().size)
    }

    /**
     * An account with nowhere to put it is told so, and nothing is
     * moved — a guessed folder name has the server create one, where
     * the message then sits invisible to every other client.
     */
    @Test
    fun an_account_with_no_trash_is_told_so() = runBlocking {
        val script = serving(
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in",
            "* LIST (\\HasNoChildren) \".\" \"INBOX\"",
            "a2 OK listed",
        )
        val outcome = MailboxActions.delete(account, row, store)
        assertTrue(outcome.toString(), outcome is MailboxActions.Outcome.Failed)
        assertFalse(script.written.toString(), script.written.any { it.contains("MOVE") })
        assertEquals(1, store.rows().size)
    }

    /** Marking unread reaches the server and this device both. */
    @Test
    fun marking_unread_tells_the_server_and_the_list() = runBlocking {
        store.saveRows(listOf(row.copy(seen = true)))
        val script = serving("* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in", "a2 OK selected", "a3 OK stored")
        val outcome = MailboxActions.markUnread(account, row, store)
        assertTrue(outcome.toString(), outcome is MailboxActions.Outcome.Done)
        assertTrue(
            script.written.toString(),
            script.written.any { it.contains("UID STORE 7 -FLAGS") },
        )
        assertFalse("the list still shows it as read", store.rows()[0].seen)
    }
}
