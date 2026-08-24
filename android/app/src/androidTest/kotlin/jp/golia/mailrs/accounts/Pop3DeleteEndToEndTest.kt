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
 * Deleting from a POP3 mailbox.
 *
 * Three things that are not true of IMAP, and each is a way to lose or
 * fail to lose a message quietly.
 */
@RunWith(AndroidJUnit4::class)
class Pop3DeleteEndToEndTest {
    private lateinit var store: AccountStore
    private val account = MailAccount.make("me@example.com", "Me", 0).copy(
        imapHost = "pop.example.com",
        imapPort = 995,
        incoming = Incoming.POP3,
    )
    private val uidl = "QhdPYR-a"
    private val row = MailboxRow(
        accountId = MailAccount.idFor("me@example.com"),
        uid = MailboxSyncRunner.foldedUid(uidl),
        folder = "INBOX", seen = false, sender = "Ada", subject = "Lunch",
        date = null, messageId = "m1",
    )

    private class Script(private val lines: MutableList<String>) : Pop3Session.Transport {
        val written = mutableListOf<String>()
        override fun readLine(): String {
            if (lines.isEmpty()) throw Pop3Session.Failure.Closed()
            return lines.removeAt(0)
        }
        override fun write(text: String) {
            written.add(text.trimEnd('\r', '\n'))
        }
        override fun close() = Unit
    }

    private fun serving(vararg lines: String): Script {
        val script = Script(lines.toMutableList())
        MailboxActions.openPop3 = { _, _ ->
            Pop3Session("localhost", 995).also { it.transport = script }
        }
        return script
    }

    @Before
    fun setUp() {
        store = AccountStore(ApplicationProvider.getApplicationContext())
        store.save(listOf(account))
        store.saveSecret("app-password", account.id)
        store.replaceRows(listOf(row))
        store.savePopSeen(account.id, setOf(uidl))
    }

    @After
    fun tearDown() {
        MailboxActions.openPop3 = { host, port -> Pop3Session(host, port) }
        store.remove(account.id)
        store.replaceRows(emptyList())
    }

    /**
     * **The number is only valid in this session**, so the uidl is
     * looked up now — a stored number would delete whatever happens to
     * be in that position today. Here the message has moved from 1 to
     * 3 since it was fetched.
     *
     * And **`DELE` does not delete**: the server acts at `QUIT`, so a
     * session dropped after `DELE` leaves the mailbox untouched.
     */
    @Test
    fun the_number_is_looked_up_now_and_quit_commits_it() = runBlocking {
        val script = serving(
            "+OK POP3 ready",
            "+OK user accepted",
            "+OK signed in",
            "+OK listing",
            "1 QhdPYR-other",
            "2 QhdPYR-another",
            "3 $uidl",
            ".",
            "+OK marked",
            "+OK bye",
        )
        val outcome = MailboxActions.delete(account, row, store)
        assertTrue(outcome.toString(), outcome is MailboxActions.Outcome.Done)
        assertTrue(script.written.toString(), script.written.any { it == "DELE 3" })
        assertTrue("DELE without QUIT deletes nothing", script.written.any { it == "QUIT" })
        assertTrue(store.rows().isEmpty())
    }

    /**
     * **A message already gone is a success.** It was deleted from
     * another device, and telling somebody their delete failed when
     * the thing is gone is a lie that makes them try again.
     */
    @Test
    fun a_message_already_gone_is_not_an_error() = runBlocking {
        val script = serving(
            "+OK POP3 ready",
            "+OK user accepted",
            "+OK signed in",
            "+OK listing",
            "1 QhdPYR-other",
            ".",
            "+OK bye",
        )
        val outcome = MailboxActions.delete(account, row, store)
        assertTrue(outcome.toString(), outcome is MailboxActions.Outcome.Done)
        // Nothing was marked, because there was nothing to mark.
        assertTrue(script.written.toString(), script.written.none { it.startsWith("DELE") })
        assertTrue(store.rows().isEmpty())
    }

    /** A refused sign-in leaves the row alone and says why. */
    @Test
    fun a_refused_sign_in_leaves_the_row() = runBlocking {
        serving("+OK POP3 ready", "+OK user accepted", "-ERR [AUTH] Invalid password")
        val outcome = MailboxActions.delete(account, row, store)
        assertTrue(outcome.toString(), outcome is MailboxActions.Outcome.Failed)
        assertEquals(1, store.rows().size)
    }
}
