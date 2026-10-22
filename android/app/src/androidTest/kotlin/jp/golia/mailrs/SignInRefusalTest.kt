package jp.golia.mailrs

import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A wrong password is a 401 with no body. It refuses what was typed; there
 * was never a session to lose, so the screen says the password did not
 * match rather than that the user was signed out.
 */
@RunWith(AndroidJUnit4::class)
class SignInRefusalTest : MailrsUiTest() {
    @Test
    fun a_wrong_password_says_the_password_did_not_match() {
        compose.activityRule.scenario.onActivity { it.useStubServer(stubBase()) }
        compose.onNodeWithTag("field.address").performTextInput("me@golia.jp")
        compose.onNodeWithTag("field.password").performTextInput("wrong-password")
        compose.onNodeWithTag("button.signIn").performClick()

        scrollToTag("text.signInError", "a wrong password was not refused on screen")
        compose.onNodeWithTag("text.signInError")
            .assertTextContains("address and password did not match", substring = true)
        assertTrue(
            "a wrong password left the sign-in screen",
            compose.onAllNodes(hasTestTag("field.address")).fetchSemanticsNodes().isNotEmpty(),
        )
    }
}
