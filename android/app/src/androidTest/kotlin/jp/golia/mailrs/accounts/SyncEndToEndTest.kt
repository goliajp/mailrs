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
 * The wire joined to the store.
 *
 * Every rule above the socket is asserted somewhere, and every socket
 * conversation is asserted against a scripted transport — and until
 * this existed the two had never been checked **together**. A pass that
 * talks to the server correctly and files the answer in the wrong place
 * passes both halves and shows nobody their mail.
 *
 * Instrumented rather than a JVM test because the credential goes
 * through the Android Keystore, which only exists on a device: a fake
 * store here would be a test of the fake.
 */
@RunWith(AndroidJUnit4::class)
class SyncEndToEndTest {
    private lateinit var store: AccountStore
    private val account = MailAccount.make("me@example.com", "Me", 0).copy(
        imapHost = "imap.example.com",
        imapPort = 993,
    )

    /** A server that says exactly what it is told to say. */
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
        store.saveMarks(emptyMap())
    }

    @After
    fun tearDown() {
        MailboxSyncRunner.openImap = { host, port -> ImapSession(host, port) }
        MailboxSyncRunner.now = { System.currentTimeMillis() / 1000 }
        store.remove(account.id)
        store.saveRows(emptyList())
        store.saveMarks(emptyMap())
    }

    private fun header(subject: String) =
        "From: Ada <ada@example.com>\r\nSubject: $subject\r\n" +
            "Date: Sun, 24 Aug 2025 01:46:40 +0000\r\nMessage-ID: <m7@example.com>\r\n\r\n"

    /**
     * One pass, from the greeting to a row a list could show. The
     * subject arrives encoded, because that is how a non-ASCII one
     * always does.
     */
    @Test
    fun a_pass_puts_a_readable_row_in_the_store() = runBlocking {
        val body = header("=?utf-8?B?5Lya6K2w?=")
        serving(
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in",
            "* LIST (\\HasNoChildren) \".\" \"INBOX\"",
            "a2 OK listed",
            "* 1 EXISTS",
            "* OK [UIDVALIDITY 42] valid",
            "a3 OK selected",
            "* 1 FETCH (UID 7 FLAGS () BODY[HEADER] {" + body.length + "}",
            body + ")",
            "a4 OK fetched",
        )
        val outcome = MailboxSyncRunner.run(account, store)

        assertEquals(outcome.failure, null, outcome.failure)
        assertEquals(1, outcome.fetched)

        val rows = store.rows()
        assertEquals(1, rows.size)
        // Decoded, not `=?utf-8?B?...?=` — a row shows what somebody
        // wrote, and the decoding happens far from here.
        assertEquals("会議", rows[0].subject)
        assertFalse("a message with no Seen flag arrived read", rows[0].seen)
        assertEquals("INBOX", rows[0].folder)
        assertEquals(7L, rows[0].uid)
        assertEquals(account.id, rows[0].accountId)

        // And the place is remembered, or the next pass fetches it all
        // over again.
        assertEquals(42L, store.marksFor(account.id)["INBOX"]?.uidValidity)
        assertEquals(7L, store.marksFor(account.id)["INBOX"]?.highestUid)
    }

    /**
     * The second pass asks only for what is new — the whole point of
     * remembering a place — and applies what it learns about the one
     * already here.
     */
    @Test
    fun a_second_pass_asks_only_for_what_is_new() = runBlocking {
        store.saveMarksFor(account.id, mapOf("INBOX" to FolderMark(42L, 7L)))
        store.saveRows(
            listOf(
                MailboxRow(
                    accountId = account.id, uid = 7L, folder = "INBOX", seen = false,
                    sender = "Ada", subject = "old", date = null, messageId = "m7",
                ),
            ),
        )
        val script = serving(
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in",
            "* LIST (\\HasNoChildren) \".\" \"INBOX\"",
            "a2 OK listed",
            "* OK [UIDVALIDITY 42] valid",
            "a3 OK selected",
            "a4 OK nothing new",
            "* 1 FETCH (UID 7 FLAGS (\\Seen))",
            "a5 OK flags",
        )
        MailboxSyncRunner.run(account, store)

        val fetch = script.written.first { it.contains("UID FETCH") && it.contains("BODY.PEEK") }
        assertTrue(fetch, fetch.contains("8:*"))

        // A message read on a laptop stops being bold here.
        assertTrue("the flag learned from the server was not applied", store.rows()[0].seen)
    }

    /**
     * A server that refuses the credential must leave the store alone
     * and say why — an account that quietly fetches nothing is
     * indistinguishable from an account with no new mail.
     */
    @Test
    fun a_refused_sign_in_says_so_and_changes_nothing() = runBlocking {
        serving("a1 NO [AUTHENTICATIONFAILED] Invalid credentials")
        val outcome = MailboxSyncRunner.run(account, store)
        assertTrue("a refused sign-in was reported as a pass", outcome.failure != null)
        assertTrue(store.rows().isEmpty())
        assertTrue(store.marksFor(account.id).isEmpty())
    }

    /**
     * **The timestamp is written only by a pass that worked.**
     *
     * "No new mail" and "we have not managed to check since yesterday"
     * look identical on screen, and the line that tells them apart is
     * worse than useless if a failed pass sets it — the screen would
     * then say "just now" about mail it never got.
     */
    @Test
    fun a_failed_pass_does_not_claim_to_have_checked() = runBlocking {
        MailboxSyncRunner.now = { 1_756_000_000L }
        serving(
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 NO [AUTHENTICATIONFAILED] Invalid credentials",
        )
        MailboxSyncRunner.run(account, store)
        assertEquals(null, store.lastSync(account.id))

        // And a pass that works does set it.
        val body = header("Hello")
        serving(
            "* OK [CAPABILITY IMAP4rev1] ready",
            "a1 OK signed in",
            "* LIST (\\HasNoChildren) \".\" \"INBOX\"",
            "a2 OK listed",
            "* OK [UIDVALIDITY 42] valid",
            "a3 OK selected",
            "* 1 FETCH (UID 7 FLAGS () BODY[HEADER] {" + body.length + "}",
            body + ")",
            "a4 OK fetched",
        )
        MailboxSyncRunner.run(account, store)
        assertEquals(1_756_000_000L, store.lastSync(account.id))
    }
}
