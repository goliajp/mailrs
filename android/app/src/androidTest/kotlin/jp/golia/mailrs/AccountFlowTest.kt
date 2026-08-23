package jp.golia.mailrs

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Connecting a mailbox somewhere else.
 *
 * The screen shipped on all three clients with no UI test on any of
 * them, because the shared stub did not answer `/api/accounts/external`
 * at all — so the list, the reason a broken account gives, and the
 * manual server form were only ever exercised by hand. On this
 * platform the Connect button was an empty lambda for the whole of
 * that time and nothing said so.
 */
@RunWith(AndroidJUnit4::class)
class AccountFlowTest : MailrsUiTest() {

    private fun openAccounts() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("settings.mailAccounts", "settings never listed the mail accounts row")
        compose.onNodeWithTag("settings.mailAccounts").performScrollTo().performClick()
    }

    @Test
    fun the_connected_mailboxes_are_listed() {
        openAccounts()
        waitForTag("account.acc_gmail", "the account list never decoded")
        compose.onNodeWithText("someone@gmail.com").assertIsDisplayed()
        compose.onNodeWithText("someone@qq.com").assertIsDisplayed()
    }

    /**
     * The reason, on the screen somebody reads. The web had it in a
     * hover tooltip and both phones had it nowhere.
     */
    @Test
    fun a_broken_account_says_why() {
        openAccounts()
        waitForTag("account.why.acc_qq", "an account that stopped syncing said nothing about it")
        compose.onNodeWithTag("account.why.acc_qq").assertIsDisplayed()
    }

    /**
     * `Paused` was a state with a reader and no writer: `is_due`
     * honoured it, the list rendered it, and nothing anywhere could
     * set it — so a noisy account could only be deleted, which throws
     * away the credential to achieve something temporary.
     *
     * Not offered for a rejected credential, which is the second half
     * of the rule and the half a test can tell apart from "the button
     * is missing".
     */
    @Test
    fun an_account_can_be_paused_but_a_refused_one_cannot() {
        openAccounts()
        waitForTag("account.pause.acc_gmail", "no way to pause an account")
        assertTrue(
            "pausing was offered for an account whose credential was refused",
            compose.onAllNodes(hasTestTag("account.pause.acc_qq"))
                .fetchSemanticsNodes().isEmpty(),
        )
    }

    /**
     * Autodiscovery covers the providers people use; a company server
     * with no SRV record and no ISPDB entry needs the boxes. They were
     * in the API from the first day and no client ever sent them.
     */
    @Test
    fun the_servers_can_be_typed_in() {
        openAccounts()
        waitForTag("account.email", "no form to fill")
        compose.onNodeWithTag("account.email").performTextInput("me@internal.example.jp")
        waitForTag("account.secret", "the secret field never appeared")
        compose.onNodeWithTag("account.secret").performTextInput("hunter2")

        // Shut by default: a form that opens with eight empty boxes
        // teaches everybody that connecting mail is hard.
        assertTrue(
            "the boxes were open before anybody asked for them",
            compose.onAllNodes(hasTestTag("account.incoming.host"))
                .fetchSemanticsNodes().isEmpty(),
        )
        compose.onNodeWithTag("account.manual").performScrollTo().performClick()
        waitForTag("account.incoming.host", "the boxes never opened")
        compose.onNodeWithTag("account.incoming.host")
            .performTextInput("imap.internal.example.jp")
        compose.onNodeWithTag("account.incoming.port").performTextInput("993")
        compose.onNodeWithTag("account.outgoing.host")
            .performTextInput("smtp.internal.example.jp")
        compose.onNodeWithTag("account.outgoing.port").performTextInput("587")

        compose.onNodeWithTag("account.connect").performScrollTo().performClick()
        // The row the stub echoes back is the proof it left the phone:
        // the Connect button used to be an empty lambda, and a
        // composer that closes proves nothing about what was sent.
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("account.acc_new").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onAllNodesWithTag("account.acc_new").onFirst().assertIsDisplayed()
    }
}
