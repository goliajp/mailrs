package jp.golia.mailrs

import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The sign-in address must be a whole address before it is sent: half of
 * one is said on the spot rather than answered by the server as a wrong
 * password.
 */
@RunWith(AndroidJUnit4::class)
class SignInAddressTest : MailrsUiTest() {
    private val hint = "Enter the full address, like name@example.com"

    private fun hintsShown() = compose.onAllNodesWithText(hint).fetchSemanticsNodes().size

    @Test
    fun a_partial_address_is_said_and_cannot_be_sent() {
        assertEquals("no hint on an empty field", 0, hintsShown())
        compose.onNodeWithTag("field.password").performTextInput("anything")

        compose.onNodeWithTag("field.address").performTextInput("me@golia")
        compose.waitForIdle()
        assertEquals("the hint for a partial address", 1, hintsShown())
        compose.onNodeWithTag("button.signIn").assertIsNotEnabled()

        compose.onNodeWithTag("field.address").performTextClearance()
        compose.onNodeWithTag("field.address").performTextInput("me@golia.jp")
        compose.waitForIdle()
        assertEquals("no hint once it is whole", 0, hintsShown())
        compose.onNodeWithTag("button.signIn").assertIsEnabled()
    }
}
