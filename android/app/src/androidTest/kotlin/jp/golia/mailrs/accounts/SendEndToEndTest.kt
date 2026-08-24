package jp.golia.mailrs.accounts

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Sending, builder joined to wire.
 *
 * `OutgoingMessage` is tested and `SmtpSession` is tested, and the seam
 * between them is where **a Bcc leaks**: the address belongs in
 * `RCPT TO` and nowhere in the DATA block, and only a look at what
 * actually went out can say that it is so.
 */
@RunWith(AndroidJUnit4::class)
class SendEndToEndTest {
    private lateinit var store: AccountStore
    private val account = MailAccount.make("me@example.com", "Me", 0).copy(
        smtpHost = "smtp.example.com",
        smtpPort = 465,
    )

    private class Script(private val lines: MutableList<String>) : SmtpSession.Transport {
        val written = mutableListOf<String>()
        override fun readLine(): String {
            if (lines.isEmpty()) throw SmtpSession.Failure.Closed()
            return lines.removeAt(0)
        }
        override fun write(text: String) {
            written.add(text)
        }
        override fun close() = Unit
    }

    private fun serving(vararg lines: String): Script {
        val script = Script(lines.toMutableList())
        AccountSender.openSmtp = { _, _ ->
            SmtpSession("localhost", 465).also { it.transport = script }
        }
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
        AccountSender.openSmtp = { host, port -> SmtpSession(host, port) }
        store.remove(account.id)
    }

    /**
     * A server that accepts everything, for [recipients] of them.
     *
     * The count is a parameter because a scripted server answers in
     * order: one `250` too many and `DATA` reads it instead of the
     * `354`, and the whole exchange slides by one. A fixed script that
     * happened to fit the first test failed the second, which is the
     * script disagreeing with the message rather than either being
     * wrong.
     */
    private fun exchange(recipients: Int) = buildList {
        add("220 smtp.example.com ESMTP")
        add("250 smtp.example.com")
        add("235 2.7.0 Accepted")
        add("250 2.1.0 sender ok")
        repeat(recipients) { add("250 2.1.5 recipient ok") }
        add("354 go ahead")
        add("250 2.0.0 queued")
        add("221 bye")
    }.toTypedArray()

    /**
     * **The blind copy stays blind.** Its address is offered to the
     * server as a recipient and appears nowhere in what the recipients
     * receive — which is the whole of what "blind" means, and a
     * mistake nobody can take back.
     */
    @Test
    fun a_blind_copy_is_in_the_envelope_and_not_in_the_message() = runBlocking {
        val script = serving(*exchange(3))
        val draft = OutgoingMessage.Draft(
            from = account.address,
            to = listOf("you@example.com"),
            cc = listOf("cc@example.com"),
            subject = "Lunch",
            body = "hello",
        )
        val outcome = AccountSender.send(draft, account, store, listOf("secret@example.com"))
        assertTrue(outcome.toString(), outcome is AccountSender.Outcome.Sent)

        // Offered to the server, so it is delivered.
        assertTrue(
            script.written.toString(),
            script.written.any { it.startsWith("RCPT TO:<secret@example.com>") },
        )
        // And absent from what anybody receives.
        val data = dataBlock(script)
        assertFalse("the blind copy was written into the message", data.contains("secret@example.com"))
        assertFalse("a Bcc header was written", data.lowercase().contains("bcc:"))
        assertTrue("the Cc header is missing", data.contains("Cc: cc@example.com"))
    }

    /**
     * A body line of a single dot would end the DATA block. Left
     * unstuffed, the message arrives cut in half — and the half that
     * arrives looks like a whole message.
     */
    @Test
    fun a_dot_in_the_body_survives_the_send() = runBlocking {
        val script = serving(*exchange(1))
        val draft = OutgoingMessage.Draft(
            from = account.address,
            to = listOf("you@example.com"),
            subject = "Recipe",
            body = "boil water\n.\nserve",
        )
        AccountSender.send(draft, account, store)
        val data = dataBlock(script)
        assertTrue(data, data.contains("\r\n..\r\n"))
        // The terminator is its own write now — `dataBlock` stops at
        // it by construction, so the assertion is that it was sent,
        // and last of the block.
        val beforeQuit = script.written.filterNot { it.startsWith("QUIT") }.last()
        assertTrue("the block was never terminated", beforeQuit == ".\r\n")
    }

    /**
     * The envelope sender is the account's own address. A server that
     * permits one address will refuse another, and SPF makes that
     * refusal correct.
     */
    @Test
    fun the_envelope_sender_is_the_account() = runBlocking {
        val script = serving(*exchange(1))
        val draft = OutgoingMessage.Draft(
            from = "someone@else.example",
            to = listOf("you@example.com"),
            subject = "x",
            body = "y",
        )
        AccountSender.send(draft, account, store)
        assertTrue(
            script.written.toString(),
            script.written.any { it.startsWith("MAIL FROM:<me@example.com>") },
        )
    }

    /**
     * Everything between `DATA` and the terminator, joined.
     *
     * The message is streamed, so headers, body and each base64 line
     * are separate writes — an assertion about which chunk holds what
     * is an assertion about the chunking, and the chunking is not the
     * protocol.
     *
     * **Only the block**, not every write: joining the whole session
     * would fold the `RCPT TO:` lines into "the message", and an
     * assertion that a Bcc address is absent from the message would
     * then find it in the envelope and call that a leak.
     */
    private fun dataBlock(script: Script): String {
        val from = script.written.indexOfFirst { it.startsWith("DATA") }
        val body = script.written.drop(from + 1)
        val to = body.indexOfFirst { it == ".\r\n" }
        return when (to) {
            -1 -> body.joinToString("")
            else -> body.take(to).joinToString("")
        }
    }
}
