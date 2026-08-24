package jp.golia.mailrs.accounts

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.net.InetSocketAddress
import java.net.Socket
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The one thing no other test in this repo does: reach a mail server
 * over a **real socket, with real TLS, and a certificate the app
 * actually validates**.
 *
 * Every other IMAP and SMTP assertion here is made against a scripted
 * transport — a fake that hands the session lines from a list. That
 * covers the conversation and nothing under it: the handshake, the
 * certificate check, the hostname check, the socket's own framing.
 *
 * The app is not modified for this. It validates the certificate as it
 * always does; the **debug build** is told to trust one more authority
 * through `debug-overrides`, which the platform ignores in a release
 * build. `scripts/android-build.sh` generates the authority per run.
 *
 * Skipped when the stub is not listening, so a run from Android Studio
 * without the script does not report a failure about a server that was
 * never started — an absent measuring device must not look like data.
 */
@RunWith(AndroidJUnit4::class)
class RealTlsMailTest {
    private lateinit var store: AccountStore

    private val account = MailAccount.make("me@example.com", "Me", 0).copy(
        // `adb reverse` makes the host's ports guest-local, so this is
        // loopback rather than 10.0.2.2 — the same reason the HTTP stub
        // is reached that way.
        imapHost = "127.0.0.1",
        imapPort = IMAPS,
        smtpHost = "127.0.0.1",
        smtpPort = SUBMISSION,
    )

    private fun listening(port: Int): Boolean = runCatching {
        Socket().use { it.connect(InetSocketAddress("127.0.0.1", port), 500) }
        true
    }.getOrDefault(false)

    @Before
    fun setUp() {
        assumeTrue("the TLS mail stub is not listening", listening(IMAPS))
        store = AccountStore(InstrumentationRegistry.getInstrumentation().targetContext)
        store.replaceRows(emptyList())
        store.saveMarks(emptyMap())
        store.save(listOf(account))
        store.saveSecret("app-password", account.id)
    }

    @After
    fun tearDown() {
        if (::store.isInitialized) {
            store.remove(account.id)
            store.replaceRows(emptyList())
            store.saveMarks(emptyMap())
        }
    }

    /** A whole pass: TLS to a real listener, and rows a list could show. */
    @Test
    fun a_pass_over_real_tls_fills_the_store() = runBlocking {
        val outcome = MailboxSyncRunner.run(account, store)
        assertNull(outcome.failure, outcome.failure)
        assertEquals(2, outcome.fetched)
        val rows = store.rows().sortedBy { it.uid }
        assertEquals(listOf(1001L, 1002L), rows.map { it.uid })
        // Decoded on the way through, which is what makes this a test
        // of the chain rather than of the socket: the subject crossed
        // the wire as an RFC 2047 encoded word.
        assertEquals("会議", rows[0].subject)
        assertEquals("Ada", rows[0].sender)
    }

    /**
     * Plaintext submission, `STARTTLS`, and the connection upgraded in
     * place.
     *
     * The stub refuses `AUTH` before the upgrade, so a client that
     * skipped it would fail here rather than quietly send a password in
     * the clear.
     */
    @Test
    fun a_message_goes_out_through_starttls() = runBlocking {
        assumeTrue(listening(SUBMISSION))
        val draft = OutgoingMessage.Draft(
            from = account.address,
            to = listOf("you@example.com"),
            subject = "会議のご案内",
            body = "本文です。\n.\n終わり",
        )
        val outcome = AccountSender.send(draft, account, store)
        assertEquals(AccountSender.Outcome.Sent, outcome)
    }

    /**
     * **A certificate this device does not trust is an error, not a
     * wait.**
     *
     * The sibling assertion on iOS found a real defect: a refused
     * handshake left the client sitting in `NWConnection`'s `.waiting`
     * state indefinitely, so an expired certificate or a proxy in the
     * middle gave an app that hung rather than one that said what was
     * wrong. This is the same question asked of the other platform's
     * socket layer.
     *
     * The bound is well inside the 20-second connect timeout, because
     * a test that only checks "it eventually failed" passes on exactly
     * that defect.
     */
    @Test
    fun an_untrusted_certificate_fails_rather_than_hangs() = runBlocking {
        assumeTrue(listening(UNTRUSTED))
        val rogue = account.copy(imapPort = UNTRUSTED)
        store.save(listOf(rogue))
        store.saveSecret("app-password", rogue.id)
        val began = System.currentTimeMillis()
        val outcome = MailboxSyncRunner.run(rogue, store)
        val took = System.currentTimeMillis() - began
        assertNotNull("an untrusted certificate was accepted", outcome.failure)
        assertTrue("it took ${took}ms — that is the timeout, not a refusal", took < 10_000)
    }

    private companion object {
        const val IMAPS = 9993
        const val SUBMISSION = 9587
        const val UNTRUSTED = 9994
    }
}
