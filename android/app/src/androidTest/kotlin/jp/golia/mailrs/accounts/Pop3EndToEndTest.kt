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
 * A POP3 account, wire joined to store.
 *
 * POP3's rules are the ones that cost data when they are wrong: the
 * only durable identity is the uidl, the seen-set must be pruned to
 * what the server still has, and the session holds an exclusive lock
 * until it is ended. None of those can be shown by the session or by
 * the plan alone.
 */
@RunWith(AndroidJUnit4::class)
class Pop3EndToEndTest {
    private lateinit var store: AccountStore
    private val account = MailAccount.make("me@example.com", "Me", 0).copy(
        imapHost = "pop.example.com",
        imapPort = 995,
        incoming = Incoming.POP3,
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
        MailboxSyncRunner.openPop3 = { _, _ ->
            Pop3Session("localhost", 995).also { it.transport = script }
        }
        return script
    }

    @Before
    fun setUp() {
        store = AccountStore(ApplicationProvider.getApplicationContext())
        store.save(listOf(account))
        store.saveSecret("app-password", account.id)
        store.replaceRows(emptyList())
        store.savePopSeen(account.id, emptySet())
    }

    @After
    fun tearDown() {
        MailboxSyncRunner.openPop3 = { host, port -> Pop3Session(host, port) }
        store.remove(account.id)
        store.replaceRows(emptyList())
    }

    private val header = "From: Ada <ada@example.com>\r\n" +
        "Subject: =?utf-8?B?5Lya6K2w?=\r\n" +
        "Date: Sun, 24 Aug 2025 01:46:40 +0000\r\n" +
        "Message-ID: <m1@example.com>\r\n"

    /**
     * One pass: the headers only, the uidl as identity, and the
     * session ended so the mailbox is not left locked.
     */
    @Test
    fun a_pass_reads_headers_and_ends_the_session() = runBlocking {
        val script = serving(
            "+OK POP3 ready",
            "+OK user accepted",
            "+OK signed in",
            "+OK listing",
            "1 QhdPYR-a",
            ".",
            "+OK top",
            *header.trimEnd('\r', '\n').split("\r\n").toTypedArray(),
            "",
            ".",
            "+OK bye",
        )
        val outcome = MailboxSyncRunner.run(account, store)
        assertEquals(outcome.failure, null, outcome.failure)

        val rows = store.rows()
        assertEquals(1, rows.size)
        assertEquals("会議", rows[0].subject)
        // POP3 has no server-side flags, so everything arrives unread
        // and only this device can ever say otherwise.
        assertFalse(rows[0].seen)
        assertEquals("INBOX", rows[0].folder)

        // Headers only: fetching whole messages to show a list
        // downloads the mailbox to display it.
        assertTrue(script.written.toString(), script.written.any { it == "TOP 1 0" })
        assertFalse(script.written.toString(), script.written.any { it.startsWith("RETR") })

        // **And QUIT.** A POP3 server holds an exclusive lock for the
        // length of a session; one dropped without it makes the
        // mailbox unreadable on the person's other device until the
        // timeout.
        assertTrue(script.written.toString(), script.written.any { it == "QUIT" })

        // The uidl is what is remembered, because the numbers are
        // renumbered on every session.
        assertEquals(setOf("QhdPYR-a"), store.popSeen(account.id))
    }

    /**
     * A second pass fetches nothing it has seen, and **forgets the
     * uidls the server no longer has** — otherwise a year of
     * bookkeeping outgrows the mailbox it is about.
     */
    @Test
    fun a_second_pass_skips_what_it_has_and_forgets_what_is_gone() = runBlocking {
        store.savePopSeen(account.id, setOf("QhdPYR-a", "QhdPYR-gone"))
        val script = serving(
            "+OK POP3 ready",
            "+OK user accepted",
            "+OK signed in",
            "+OK listing",
            "1 QhdPYR-a",
            ".",
            "+OK bye",
        )
        MailboxSyncRunner.run(account, store)

        assertFalse(script.written.toString(), script.written.any { it.startsWith("TOP") })
        assertEquals(setOf("QhdPYR-a"), store.popSeen(account.id))
    }

    /**
     * A refused password says so and changes nothing — POP3 has no
     * code for it, so the words are all there is, and a client that
     * checks only the `USER` reply signs in to nothing.
     */
    @Test
    fun a_refused_password_says_so() = runBlocking {
        serving(
            "+OK POP3 ready",
            "+OK user accepted",
            "-ERR [AUTH] Invalid password",
        )
        val outcome = MailboxSyncRunner.run(account, store)
        assertTrue("a refused password was reported as a pass", outcome.failure != null)
        assertTrue(store.rows().isEmpty())
        assertTrue(store.popSeen(account.id).isEmpty())
    }
}
