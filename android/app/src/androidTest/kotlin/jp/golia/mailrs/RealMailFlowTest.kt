package jp.golia.mailrs

import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.net.InetSocketAddress
import java.net.Socket
import java.net.URL
import jp.golia.mailrs.accounts.MailAccount
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The whole of it, through the screens: add a mailbox somewhere else
 * and have a **real mail server over TLS** accept the credential.
 *
 * [MailboxFlowTest] checks everything up to the connection, because
 * when it was written there was no IMAP to connect to. There is now:
 * `ios/Testing/tls-mail-stub.py`, reached over a real socket with a
 * certificate the **debug build** has been told to trust through
 * `debug-overrides` — which the platform ignores in a release build,
 * and which leaves the app's own validation exactly as it ships.
 *
 * This closes the last item in the audit's coverage accounting: no
 * test on any platform had gone through adding a third-party mailbox
 * in the interface.
 *
 * Skipped when the stub is not listening, so a run from Android Studio
 * without the script does not report a failure about a server that was
 * never started — an absent measuring device must not look like data.
 */
@RunWith(AndroidJUnit4::class)
class RealMailFlowTest : MailrsUiTest() {

    private fun listening(port: Int): Boolean = runCatching {
        Socket().use { it.connect(InetSocketAddress("127.0.0.1", port), 500) }
        true
    }.getOrDefault(false)

    private fun openMailboxes() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        scrollToTag("settings.mailAccounts", "settings never listed the mailboxes row")
        compose.onNodeWithTag("settings.mailAccounts").performClick()
        waitForTag("account.address", "the mailboxes screen never opened")
    }

    /** Cleared first: the manual boxes open **filled in** with a guess. */
    private fun retype(tag: String, text: String) {
        compose.onNodeWithTag(tag).performTextClearance()
        compose.onNodeWithTag(tag).performTextInput(text)
    }

    @Test
    fun a_mailbox_is_added_against_a_real_tls_server() {
        assumeTrue("the TLS mail stub is not listening", listening(IMAPS))
        openMailboxes()

        compose.onNodeWithTag("account.address").performTextInput("me@example.com")
        waitForTag("account.secret", "no secret was asked for")
        compose.onNodeWithTag("account.secret").performTextInput("app-password")

        scrollToTag("account.manual", "the manual toggle never appeared")
        compose.onNodeWithTag("account.manual").performClick()
        scrollToTag("account.incoming.host", "the server boxes never opened")

        retype("account.incoming.host", "127.0.0.1")
        retype("account.incoming.port", "$IMAPS")
        retype("account.outgoing.host", "127.0.0.1")
        retype("account.outgoing.port", "$SUBMISSION")

        scrollToTag("account.add", "the Add button never appeared")
        compose.onNodeWithTag("account.add").performClick()

        // The row appears only when the server accepted the
        // credential. Asserting on it rather than on the form having
        // closed says the account was **kept**, which is what has to be
        // true for anything after this to work.
        val id = MailAccount.idFor("me@example.com")
        try {
            compose.waitUntil(20_000) {
                compose.onAllNodes(hasTestTag("account.$id")).fetchSemanticsNodes().isNotEmpty()
            }
        } catch (e: Throwable) {
            val why = runCatching {
                compose.onNodeWithTag("account.failure").fetchSemanticsNode()
                    .config.toString()
            }.getOrDefault("no reason shown")
            throw AssertionError("no account was kept — $why", e)
        }

        // --- and now write one and send it ---------------------------
        //
        // The composer closing is a fact about the composer. What has
        // to be true is that a message crossed the socket, and the only
        // place that is knowable is the server — so this asks it.
        val before = receivedCount()

        // Back, not a Done button: on this platform the mailboxes
        // screen replaces the settings screen in place rather than
        // arriving as a sheet, so there is nothing to dismiss.
        pressBack()
        scrollToTag("settings.mergedMail", "settings never listed the other-mail row")
        compose.onNodeWithTag("settings.mergedMail").performClick()
        waitForTag("mail.compose", "there was no way to write a message")
        compose.onNodeWithTag("mail.compose").performClick()

        waitForTag("compose.to", "the composer never opened")
        compose.onNodeWithTag("compose.to").performTextInput("you@example.com")
        compose.onNodeWithTag("compose.subject").performTextInput("Lunch")
        compose.onNodeWithTag("compose.body").performTextInput("Half twelve?")
        compose.onNodeWithTag("compose.send").performClick()

        // Closed or still open: two different failures. Closed means
        // the app believed it sent and the message is missing on the
        // wire; still open means the button did nothing, which is what
        // `send()`'s `from ?: return` does when there is no account
        // chosen. Only one of them is about the network.
        // **`compose.waitUntil`, not a `Thread.sleep` loop.** A
        // Compose test drives the app's frame clock; sleeping the
        // instrumentation thread does not advance it, so the coroutine
        // `send()` launched never runs and the assertion reads
        // "nothing reached the server" about a send that was never
        // allowed to start. Three runs failed that way before the
        // difference was measured — the message itself arrives in half
        // a second.
        var arrived = before
        runCatching {
            compose.waitUntil(20_000) {
                arrived = receivedCount()
                arrived != before
            }
        }

        // What the composer said, if it said anything: "nothing
        // arrived" and "the send was refused and told you so" are
        // different failures, and only one of them is about the wire.
        val said = runCatching {
            compose.onAllNodesWithTag("compose.failure").fetchSemanticsNodes()
                .firstOrNull()?.config?.toString() ?: "the composer reported nothing"
        }.getOrDefault("could not read the composer")
        // Printed, not asserted: the numbers are what say whether the
        // earlier red was a slow send or something else, and a bound
        // guessed before measuring is a bound that makes the next
        // failure look like a flake.
        assertEquals(
            "nothing reached the server — $said",
            before + 1, arrived,
        )
        assertTrue(
            "what arrived was not the message that was written",
            lastReceived().contains("Subject: Lunch"),
        )
    }

    /**
     * What the mail stub has taken, over its plain-HTTP window.
     *
     * Not TLS: this is the test asking the server what it saw, which is
     * a different conversation from the one under test.
     */
    private fun probe(): JSONObject = runCatching {
        JSONObject(URL("http://127.0.0.1:$PROBE/received").readText())
    }.getOrDefault(JSONObject())

    private fun receivedCount() = probe().optInt("count", -1)

    private fun lastReceived() = probe().optString("last", "")

    private companion object {
        const val IMAPS = 9993
        const val SUBMISSION = 9587
        const val PROBE = 9995
    }
}
