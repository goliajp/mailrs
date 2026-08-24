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
 * Opening a message, wire joined to screen.
 *
 * Opening one costs what the message weighs. A 25 MB attachment is
 * 25 MB to fetch, and fetching it to show two lines of text — on
 * somebody's mobile data, without asking — is noticed on a bill rather
 * than on a screen.
 */
@RunWith(AndroidJUnit4::class)
class ReadEndToEndTest {
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
        MessageReader.pool = ImapPool(
            open = { _, _ -> ImapSession("localhost", 993).also { it.transport = script } },
        )
        return script
    }

    @Before
    fun setUp() {
        store = AccountStore(ApplicationProvider.getApplicationContext())
        store.save(listOf(account))
        store.saveSecret("app-password", account.id)
    }

    @After
    fun tearDown() {
        MessageReader.pool.dropAll()
        MessageReader.pool = ImapPool.shared
        store.remove(account.id)
        store.replaceRows(emptyList())
    }

    private fun row(size: Long?) = MailboxRow(
        accountId = account.id, uid = 7L, folder = "INBOX", seen = true,
        sender = "Ada", subject = "Lunch", date = null, messageId = "m7", size = size,
    )

    private val body = "Content-Type: text/plain\r\n\r\nHello there.\r\n"

    private fun exchange(literal: String) = arrayOf(
        "* OK ready",
        "a1 OK signed in",
        "a2 OK selected",
        "* 1 FETCH (UID 7 BODY[] {" + literal.length + "}",
        literal + ")",
        "a3 OK fetched",
    )

    /** An ordinary message is fetched whole, which is nearly all of them. */
    @Test
    fun a_small_message_is_fetched_whole() = runBlocking {
        val script = serving(*exchange(body))
        val outcome = MessageReader.load(account, row(12_000), store)
        assertTrue(outcome.toString(), outcome is MessageReader.Outcome.Ok)
        val loaded = (outcome as MessageReader.Outcome.Ok).loaded
        assertEquals("Hello there.\r\n", loaded.text)
        assertFalse("a small message was fetched partially", loaded.partial)
        val fetch = script.written.first { it.contains("BODY.PEEK") }
        assertTrue(fetch, fetch.contains("(BODY.PEEK[])"))
    }

    /**
     * A large one is begun, not fetched — and **the screen is told**,
     * because the text is usually complete while the attachment list
     * is not, and a list that is silently short is worse than one that
     * is absent.
     */
    @Test
    fun a_large_message_is_only_begun_and_says_so() = runBlocking {
        val script = serving(*exchange(body))
        val outcome = MessageReader.load(account, row(25_000_000), store)
        val loaded = (outcome as MessageReader.Outcome.Ok).loaded
        assertTrue("a large message was fetched whole without asking", loaded.partial)
        assertEquals(25_000_000L, loaded.size)
        val fetch = script.written.first { it.contains("BODY.PEEK") }
        // `<0.262144>` is RFC 3501's partial fetch, offset then length.
        assertTrue(fetch, fetch.contains("BODY.PEEK[]<0.262144>"))
    }

    /** And all of it once the reader has asked. */
    @Test
    fun asking_for_the_whole_message_fetches_it() = runBlocking {
        val script = serving(*exchange(body))
        val outcome = MessageReader.load(account, row(25_000_000), store, wholeMessage = true)
        val loaded = (outcome as MessageReader.Outcome.Ok).loaded
        assertFalse(loaded.partial)
        val fetch = script.written.first { it.contains("BODY.PEEK") }
        assertTrue(fetch, fetch.contains("(BODY.PEEK[])"))
    }
}
