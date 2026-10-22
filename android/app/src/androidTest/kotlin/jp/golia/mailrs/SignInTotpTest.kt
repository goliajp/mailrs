package jp.golia.mailrs

import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * An account with two-factor sign-in: the server answers the password
 * with a request for a code, refuses a wrong one in its own words, and
 * signs in with the right one. The stub's `totp@golia.jp` is that account.
 */
@RunWith(AndroidJUnit4::class)
class SignInTotpTest : MailrsUiTest() {
    private val hint = "Enter the six digits from your authenticator app"

    private fun hintsShown() = compose.onAllNodesWithText(hint).fetchSemanticsNodes().size

    private fun enterCode(code: String) {
        compose.onNodeWithTag("field.totp").performTextClearance()
        compose.onNodeWithTag("field.totp").performTextInput(code)
        compose.waitForIdle()
    }

    @Test
    fun the_code_is_asked_for_refused_when_wrong_and_accepted_when_right() {
        compose.activityRule.scenario.onActivity { it.useStubServer(stubBase()) }
        assertTrue(
            "the code field showed before the server asked for it",
            compose.onAllNodes(hasTestTag("field.totp")).fetchSemanticsNodes().isEmpty(),
        )
        compose.onNodeWithTag("field.address").performTextInput("totp@golia.jp")
        compose.onNodeWithTag("field.password").performTextInput("anything")
        compose.onNodeWithTag("button.signIn").performClick()

        scrollToTag("field.totp", "the server asked for a code and no code field appeared")
        assertTrue(
            "asking for a code was shown as an error",
            compose.onAllNodes(hasTestTag("text.signInError")).fetchSemanticsNodes().isEmpty(),
        )
        compose.onNodeWithTag("button.signIn").assertIsNotEnabled()

        enterCode("12345")
        assertEquals("the hint for a short code", 1, hintsShown())
        compose.onNodeWithTag("button.signIn").assertIsNotEnabled()

        enterCode("000000")
        assertEquals("no hint once the code is six digits", 0, hintsShown())
        compose.onNodeWithTag("button.signIn").performScrollTo().assertIsEnabled().performClick()
        scrollToTag("text.signInError", "a wrong code was not refused on screen")
        compose.onNodeWithTag("text.signInError").assertTextContains("invalid TOTP code", substring = true)
        assertTrue(
            "a wrong code took the code field away",
            compose.onAllNodes(hasTestTag("field.totp")).fetchSemanticsNodes().isNotEmpty(),
        )

        enterCode("123456")
        compose.onNodeWithTag("button.signIn").performScrollTo().performClick()
        waitForTag("list.conversations", "the right code did not sign in")
    }
}
