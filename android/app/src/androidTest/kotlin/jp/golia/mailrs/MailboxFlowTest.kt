package jp.golia.mailrs

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasTestTag
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
 * Adding a mailbox somewhere else.
 *
 * A screen with no test on it is a screen exercised only by hand — and
 * the last time this app had one, three defects across three platforms
 * were hiding behind it. There is no IMAP stub here, so what is
 * checked is everything up to the connection: that the form says what
 * this provider wants before anybody types it, that the server boxes
 * stay shut until asked for and open **filled in**, and that a
 * provider which refuses passwords offers no field at all rather than
 * one that cannot work.
 */
@RunWith(AndroidJUnit4::class)
class MailboxFlowTest : MailrsUiTest() {

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

    /**
     * Nothing is asked for until there is an address to ask about: a
     * secret field over an empty address has nothing to label itself
     * with.
     */
    @Test
    fun the_secret_is_not_asked_for_until_there_is_an_address() {
        openMailboxes()
        assertTrue(
            "a secret was asked for before the address",
            compose.onAllNodes(hasTestTag("account.secret")).fetchSemanticsNodes().isEmpty(),
        )
        compose.onNodeWithTag("account.address").performTextInput("someone@qq.com")
        waitForTag("account.secret", "the secret field never appeared")
    }

    /**
     * The provider's own word, and a way to go and make one. Typing a
     * login password into a field labelled 授权码 is a mistake somebody
     * recovers from; typing it into one labelled "Password" and being
     * refused is not.
     */
    @Test
    fun a_provider_that_wants_a_code_says_what_it_calls_it() {
        openMailboxes()
        compose.onNodeWithTag("account.address").performTextInput("someone@qq.com")
        waitForTag("account.secret", "the secret field never appeared")
        compose.onNodeWithText("授权码").assertIsDisplayed()
        waitForTag("account.getSecret", "no way to go and make one")
    }

    /**
     * Said at the start, rather than discovered at the end of a
     * sign-in that could not have finished — and with no field to type
     * a password that cannot work into.
     */
    @Test
    fun a_provider_that_refuses_passwords_offers_no_field() {
        openMailboxes()
        compose.onNodeWithTag("account.address").performTextInput("someone@gmail.com")
        waitForTag("account.oauthUnavailable", "Gmail did not say it refuses passwords")
        assertTrue(
            "a password field was offered for a provider that refuses them",
            compose.onAllNodes(hasTestTag("account.secret")).fetchSemanticsNodes().isEmpty(),
        )
    }

    /**
     * Shut until asked for — a form that opens with five empty boxes
     * teaches everybody that connecting mail is hard — and then
     * **filled in**, because an empty form is one somebody has to
     * research and a filled one is one they correct.
     */
    @Test
    fun the_server_boxes_open_filled_in() {
        openMailboxes()
        compose.onNodeWithTag("account.address").performTextInput("someone@qq.com")
        waitForTag("account.manual", "no way to type the servers in")
        assertTrue(
            "the boxes were open before anybody asked for them",
            compose.onAllNodes(hasTestTag("account.incoming.host")).fetchSemanticsNodes().isEmpty(),
        )
        compose.onNodeWithTag("account.manual").performScrollTo().performClick()
        waitForTag("account.incoming.host", "the boxes never opened")
        compose.onNodeWithText("imap.qq.com").assertIsDisplayed()
        compose.onNodeWithText("993").assertIsDisplayed()
    }
}
